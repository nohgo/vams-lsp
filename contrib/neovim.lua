-- Native Neovim 0.11+ setup. No additional plugin installation is needed.
-- This module also works as lua/custom/plugins/vams-lsp.lua in a vim.pack setup.
local M = {}

function M.root(bufnr)
  local filename = vim.api.nvim_buf_get_name(bufnr)
  if filename == '' then return nil end
  filename = vim.uv.fs_realpath(filename) or filename
  local dir = vim.fs.dirname(filename)
  local current = dir
  while current do
    if vim.uv.fs_stat(current .. '/.vams.json') or vim.uv.fs_stat(current .. '/.git') then return current end
    local parent = vim.fs.dirname(current)
    if parent == current then break end
    current = parent
  end
  return dir
end

function M.command()
  if vim.g.vams_lsp_command then return vim.g.vams_lsp_command end
  if vim.fn.executable('vams-lsp') == 1 then return 'vams-lsp' end
  local cargo = vim.fn.expand('~/.cargo/bin/vams-lsp')
  if vim.fn.executable(cargo) == 1 then return cargo end
end

function M.available()
  local command = M.command()
  return vim.g.enable_vams_lsp ~= false and command ~= nil and vim.fn.executable(command) == 1
end

function M.setup()
  if not M.available() then return false end
  local capabilities = vim.lsp.protocol.make_client_capabilities()
  local has_blink, blink = pcall(require, 'blink.cmp')
  if has_blink then capabilities = blink.get_lsp_capabilities(capabilities) end
  vim.lsp.config('vams_lsp', {
    cmd = { M.command() },
    filetypes = { 'verilogams' },
    root_dir = function(bufnr, on_dir)
      local root = M.root(bufnr)
      if root then on_dir(root) end
    end,
    capabilities = capabilities,
  })
  vim.lsp.enable('vams_lsp')
  return true
end

return M
