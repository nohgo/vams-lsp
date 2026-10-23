-- Native client + real Blink/LuaSnip integration, isolated from personal config.
local root = vim.fn.getcwd()
local binary = vim.env.VAMS_LSP_BIN or root .. '/target/debug/vams-lsp'
local deps = root .. '/build/test-deps/'
local blink_path = vim.env.VAMS_TEST_BLINK or deps .. 'blink.cmp'
local snippets_path = vim.env.VAMS_TEST_LUASNIP or deps .. 'LuaSnip'
local grammar_path = vim.env.VAMS_TEST_GRAMMAR or deps .. 'tree-sitter-verilog-ams'
local cadence_path = vim.env.VAMS_TEST_CADENCE or deps .. 'cadence.nvim'
assert(vim.fn.isdirectory(blink_path) == 1, 'Fetch pinned test dependencies first')
vim.opt.runtimepath:prepend(snippets_path)
vim.opt.runtimepath:prepend(blink_path)
vim.opt.runtimepath:prepend(grammar_path)
vim.opt.runtimepath:prepend(cadence_path)
vim.treesitter.language.add('verilog_ams', { path = root .. '/.test-work/parser/verilog_ams.so' })
require('verilog_ams').setup({ highlight = false, diagnostics = false })
require('luasnip').setup({})
local blink = require('blink.cmp')
blink.setup({
  fuzzy = { implementation = 'lua' },
  snippets = { preset = 'luasnip' },
  sources = { default = { 'lsp' } },
  completion = { menu = { auto_show = false }, accept = { auto_brackets = { enabled = false } } },
})
vim.g.vams_lsp_command = binary
local setup = dofile(root .. '/contrib/neovim.lua')
assert(setup.setup())
local project = vim.fn.tempname()
vim.fn.mkdir(project, 'p')
project = vim.uv.fs_realpath(project)
vim.fn.writefile({ '{}' }, project .. '/.vams.json')
vim.fn.writefile({ 'module resistor(p,n);', 'inout p,n;', 'electrical p,n;', 'parameter real r=1k;', 'endmodule' }, project .. '/resistor.va')
vim.fn.writefile({ 'module top;', '  resi', 'endmodule' }, project .. '/top.va')
vim.cmd.edit(vim.fn.fnameescape(project .. '/top.va'))
vim.bo.filetype = 'verilogams'
local buf = vim.api.nvim_get_current_buf()
local function wait(condition, message)
  assert(vim.wait(8000, condition, 10), message)
end
local client
wait(function()
  client = vim.lsp.get_clients({ bufnr = buf, name = 'vams_lsp' })[1]
  return client and client.initialized
end, 'native LSP did not attach')
local function request(method, params)
  local response = client:request_sync(method, params, 5000, buf)
  assert(response and not response.err, vim.inspect(response))
  return response.result
end
local uri = vim.uri_from_bufnr(buf)
wait(function()
  local info = request('workspace/executeCommand', { command = 'vams.showProjectInfo' })
  return info.generation > 0 and not info.indexing
end, 'indexing did not finish')
assert(not client:supports_method('textDocument/formatting'))
assert(#vim.lsp.get_clients({ bufnr = buf }) == 1)
assert(vim.uv.fs_realpath(project) == setup.root(buf))

local items = request('textDocument/completion', { textDocument = { uri = uri }, position = { line = 1, character = 6 } })
local item
for _, candidate in ipairs(items) do if candidate.label == 'resistor' then item = candidate end end
assert(item and item.insertTextFormat == 2)

-- Exercise Blink's actual LSP provider and acceptance, then LuaSnip tabstops.
-- Permit the insertion point after EOL while this headless Lua chunk is running.
vim.opt.virtualedit = 'onemore'
vim.api.nvim_win_set_cursor(0, { 2, 6 })
local function finish_test()
assert(vim.fn.mode():match('i'), 'test must execute in Insert mode')
blink.show({ providers = { 'lsp' } })
local index
wait(function()
  for i, candidate in ipairs(blink.get_items()) do
    if candidate.label == 'resistor' then index = i; return true end
  end
end, 'Blink did not offer module completion')
blink.accept({ index = index })
wait(function() return table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), '\n'):find('u_resistor', 1, true) ~= nil end,
  'completion acceptance did not expand module')
local output = table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), '\n')
assert(output:find('.p(p_signal)', 1, true), output)
assert(output:find('.n(n_signal)', 1, true), output)
assert(output:find('\n    .p(p_signal),\n    .n(n_signal)\n  );', 1, true), 'snippet indentation: ' .. output)
assert(not output:find('${', 1, true), output)
local ls = require('luasnip')
assert(ls.jumpable(1), 'no editable snippet placeholders')
ls.jump(1)
assert(ls.jumpable(1), 'second port placeholder missing')
ls.jump(1)
vim.cmd.stopinsert()

vim.api.nvim_buf_set_lines(buf, 0, -1, false, { 'module top;', '  resistor u(.p(),.n());', 'endmodule' })
-- request_sync flushes pending native document changes before a request.
local definition = request('textDocument/definition', { textDocument = { uri = uri }, position = { line = 1, character = 3 } })
assert(definition[1].uri == vim.uri_from_fname(project .. '/resistor.va'), vim.inspect(definition))
wait(function() return #vim.diagnostic.get(buf, { namespace = vim.lsp.diagnostic.get_namespace(client.id) }) == 0 end,
  'syntax diagnostics did not clear after repair')
local other = vim.api.nvim_create_namespace('cadence-coexistence')
vim.diagnostic.set(other, buf, { { lnum = 1, col = 2, message = 'independent compiler message', source = 'xmvlog' } })
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { '  resistor u(.p(),.n())' })
request('textDocument/documentSymbol', { textDocument = { uri = uri } })
wait(function() return #vim.diagnostic.get(buf, { namespace = vim.lsp.diagnostic.get_namespace(client.id) }) > 0 end,
  'missing semicolon did not produce native LSP diagnostic')
assert(#vim.diagnostic.get(buf, { namespace = other }) == 1, 'another diagnostic source was cleared')
-- The real Cadence plugin runs its fake compiler on an actual save. Native
-- syntax diagnostics and compiler diagnostics must remain independent.
local cadence = require('cadence')
cadence.setup({ command = cadence_path .. '/tests/fake-xmvlog',
  args = { '-ams', '--trace', project .. '/compiler-trace.jsonl' }, timeout_ms = 2000 })
vim.api.nvim_buf_set_lines(buf, 0, -1, false, { '// mode=errors', 'module top;', 'resistor u(.p(),.n());', 'endmodule' })
vim.cmd('silent write')
request('textDocument/documentSymbol', { textDocument = { uri = uri } })
wait(function() return not cadence.status(buf).running and #vim.diagnostic.get(buf, { namespace = cadence.status(buf).namespace }) == 1 end,
  'on-save Cadence compiler diagnostics did not arrive')
assert(#vim.diagnostic.get(buf, { namespace = vim.lsp.diagnostic.get_namespace(client.id) }) == 0)
local grammar_namespace = vim.api.nvim_get_namespaces()['verilog_ams.syntax']
assert(not grammar_namespace or #vim.diagnostic.get(buf, { namespace = grammar_namespace }) == 0, 'duplicate grammar syntax diagnostics')
client:stop()
wait(function() return client:is_stopped() end, 'server did not shut down')
assert(#vim.diagnostic.get(buf, { namespace = cadence.status(buf).namespace }) == 1, 'LSP shutdown cleared Cadence diagnostics')
cadence.disable()
vim.fn.delete(project, 'rf')
print('PASS native Neovim attachment, Blink acceptance, LuaSnip placeholders, definitions, live syntax, grammar and on-save Cadence coexistence')
vim.cmd('qa!')

end
vim.defer_fn(function()
  local ok, err = pcall(finish_test)
  if not ok then print(err); vim.cmd('cquit 1') end
end, 20)
vim.cmd.startinsert()
