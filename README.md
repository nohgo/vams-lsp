# vams-lsp

An offline **Verilog-A / core Verilog-AMS 2.4 language server**, forked from
[Veridian](https://github.com/vivekmalneedi/veridian). It uses the suite's
[AMS Tree-sitter grammar](https://github.com/nohgo/tree-sitter-verilog-ams), not a
SystemVerilog frontend. No Cadence installation or simulator license is required.

## Features

- Complete a project module into an instance with editable instance-name and
  named-port placeholders. ANSI/non-ANSI port order and escaped names are retained.
- Optional parameter overrides using declared defaults; no `localparam` overrides.
- Accepted keyword templates for modules, analog/procedural blocks, conditionals,
  loops and generate regions, with matching closing keywords and tabstops.
- Go to module, port, parameter, variable, function, branch, nature, discipline,
  and literal include definitions. Duplicate global names return multiple targets.
- Declaration hover, document/workspace symbols, scope-aware references and
  document highlights, and signature help for declared analog functions.
- Complete remaining named ports/parameters inside existing instances.
- Live syntax diagnostics with focused missing-header-semicolon and unexpected
  punctuation messages, malformed directive checks, and conservative unknown
  or duplicate named-connection warnings.
- Versioned unsaved documents, bounded background indexing, periodic external-file
  refresh, root inspection, and data-only project configuration.
- Restricted, capture-checked rename for function-local symbols, using versioned
  edits. Unsupported or ambiguous renames are rejected.

See [coverage and limits](docs/FEATURES.md). This is language tooling, not an AMS
elaborator or simulator. Correct project configuration matters for useful results.

## Install from GitHub

Requires Rust/Cargo **1.85+**, Git, and a C compiler for the initial source build:

```sh
cargo install --git https://github.com/nohgo/vams-lsp.git --branch main --locked vams-lsp
vams-lsp --version
```

Ensure Cargo's executable directory, normally `~/.cargo/bin`, is on PATH.
Only the installed executable is needed at runtime. It embeds the pinned parser;
no Node, Python, Tree-sitter CLI, external lint engine, or neighboring checkout is
needed to run it. Python is used only by tests.

The final `vams-lsp` selects this package rather than the archived upstream
package in `vendor/`. Rerun that command to update `main`; use `--rev <commit>` instead of `--branch main`
for a reproducible revision. If replacing a local `--path` installation, add
`--force` once. Use `cargo uninstall vams-lsp` to remove it. The release workflow
builds Linux x86_64 and macOS archives/checksums when a version tag is pushed;
source installation works independently of whether a release has been published.

## Neovim: native integration

Neovim **0.11+** already contains the LSP client. No additional Neovim plugin is
required. Existing Blink/nvim-cmp or native LSP completion can consume this server;
snippet-capable clients receive editable placeholders, other clients receive text.

The easiest setup is to copy [contrib/neovim.lua](contrib/neovim.lua) to
`lua/custom/plugins/vams-lsp.lua` in your configuration, then call:

```lua
require('custom.plugins.vams-lsp').setup()
```

That file uses `vim.lsp.config` and `vim.lsp.enable`, detects the binary on PATH
or `~/.cargo/bin`, discovers roots consistently with the server, and uses Blink's
capabilities if Blink is already installed. It does not install or download plugins.
It works with `vim.pack`, Lazy, or any other manager. An override is available:

```lua
vim.g.vams_lsp_command = '/absolute/path/to/vams-lsp'
```

Keep the grammar plugin enabled for `.va`/`.vams`/`.vah` filetype detection,
highlighting and folding, with **its Lua diagnostics disabled**:

```lua
require('verilog_ams').setup({ diagnostics = false })
```

The LSP supplies syntax diagnostics instead. If you disable/uninstall the server,
you can re-enable that optional grammar adapter. Set
`vim.g.enable_vams_lsp = false` before setup to skip this LSP; the contributed
`available()` helper lets your grammar configuration choose its fallback.

Existing go-to-definition, hover, references and symbol mappings work through
standard `vim.lsp.buf` APIs. For configurations without such mappings, try:

```vim
:lua vim.lsp.buf.definition()
:lua vim.lsp.buf.hover()
:lua vim.lsp.buf.references()
:lua vim.lsp.buf.document_symbol()
```

Open a module body, type `resi`, select `resistor` in completion, and accept it.
You should get an instance resembling:

```verilog
resistor u_resistor (
  .p(p_signal),
  .n(n_signal)
);
```

The instance and connections are snippet placeholders. Use your existing snippet
jump mappings; the suite's tested Neovim setup uses Ctrl-L/Ctrl-H. Acceptance is
explicit: merely typing a complete word does not rewrite the buffer.

### Keyword templates

Accepting `module` in completion at file scope inserts a module skeleton with
editable name/ports and a matching `endmodule`. Accepting `analog` on its own
module-body line inserts:

```verilog
analog begin
  // cursor here
end
```

`begin`, `if`, `for`, `while`, `initial`, `always`, and `generate` have block
templates in suitable contexts. Existing Ctrl-L/Ctrl-H snippet navigation works.
Templates require snippet support and a standalone line; completing a keyword
inside an existing statement leaves it as a word, so an existing block is not
duplicated. Clients without snippet support retain plain keyword completion.

### What syntax diagnostics can explain

Deleting a module header's semicolon produces `Missing ; after module header`
at that header, including ANSI ports and parameterized headers. Isolated extra
commas/closing delimiters get `Unexpected ,`-style messages. Parser-provided
missing tokens are named directly. Other malformed or unsupported constructs
retain the generic syntax message.

The LSP fork reuses Veridian's protocol foundation. Veridian's detailed checks
used SystemVerilog tools such as Slang, Verible or Verilator; those engines are
not an AMS compiler and are not bundled here. The focused explanations operate
on the AMS parser's results. They do not establish semantic/compiler correctness;
use the independent Cadence integration for those checks.

## Project roots and configuration

Place `.vams.json` containing `{}` at the project root to use defaults. The native
setup walks upward from the file until it finds `.vams.json` or `.git` (including
a Git worktree file), preferring `.vams.json` at the same directory. It does not
cross a closer Git boundary to inherit a distant configuration. Otherwise the
file's directory is used. Launching Neovim from elsewhere does not change this.
That unmarked fallback scans sibling source files and their includes, rather than
recursively walking a possible home directory. Add `{}` in `.vams.json` to opt
into project-wide recursive discovery.

For larger projects:

```json
{
  "source_dirs": ["models", "rtl"],
  "include_dirs": ["include", "../shared/headers"],
  "library_dirs": ["../shared/models"],
  "exclude": ["generated/**"],
  "defines": { "FAST_MODEL": "1" },
  "completion": {
    "connections": "placeholders",
    "parameter_overrides": "omit"
  }
}
```

Paths resolve relative to the configuration file. Source/library directories
provide module definitions; include directories resolve literal includes and
are not scanned indiscriminately as standalone source units. Default extensions
are `va` and `vams`; add `v` explicitly for compatible traditional Verilog sources.
Headers reached by includes and open `.vah` buffers are also available.

Default scans respect Git ignore rules, skip common build/simulation output and
nested configured projects, avoid symlink recursion, and limit indexing to 10,000
files of at most 2 MiB each. Configure `max_files`/`max_file_bytes` when needed.
Invalid configuration retains the last valid configuration and appears in project
status. The [JSON schema](schema/vams.schema.json) documents accepted keys.

Inspect roots and indexing through normal LSP commands:

```vim
:lua vim.lsp.buf_request(0, 'workspace/executeCommand', { command = 'vams.showProjectInfo' }, function(err, result) vim.print(err or result) end)
:lua vim.lsp.buf_request(0, 'workspace/executeCommand', { command = 'vams.reindex' }, function(err, result) vim.print(err or result) end)
```

Or outside the editor:

```sh
vams-lsp --print-root /path/to/model.va
vams-lsp --check-project /path/to/project
```

The latter reports discovery/configuration, not compiler correctness. Background
refresh runs every two seconds; open-buffer changes update immediately. Reindexing
does not replace unsaved text with disk content. One client/process is used per
root; dynamic multi-root workspaces are not advertised.

## Working with the rest of the suite

| Component | Owner |
| --- | --- |
| Highlighting/folding/filetype | `tree-sitter-verilog-ams` |
| Offline completion/navigation/syntax | `vams-lsp` |
| Formatting | `vamsfmt` through Conform |
| Licensed on-save compiler diagnostics/quickfix | `cadence.nvim` |

The server never invokes Cadence, Verible, Verilator, or a formatter. It does not
advertise formatting capabilities, so it cannot compete with Conform. It publishes
only its own `vams-lsp` diagnostics. Stopping it does not clear Cadence diagnostics.
It attaches only to `verilogams`, leaving an existing SystemVerilog LSP alone.

## Development and verification

```sh
make build
make test
make check
make test-protocol
sh scripts/fetch-test-deps.sh
make test-neovim
```

`make` uses `build/cargo` for its Cargo cache. Direct Cargo commands use the normal
Cargo environment. Editor tests fetch pinned Blink, LuaSnip, grammar, and Cadence
repositories into `build/test-deps`, compile the included grammar C source, and
run an isolated editor with a fake Cadence compiler. They exercise actual Blink
acceptance, placeholder jumps/indentation, native navigation and diagnostic
repair, and simultaneous grammar/Cadence behavior. Personal configuration is not
loaded by these tests. Rust tests cover extraction, scopes, root selection,
include discovery, versions, Unicode/CRLF, snippets and limits; Python tests
exercise the actual stdio server, lifecycle, capabilities and external file changes.

The runtime is locally verified with Rust 1.85.1 and Neovim 0.11.5/0.12.5 on
macOS arm64. Linux/macOS CI is configured separately; real licensed Cadence
compilation is not claimed by the fake-compiler integration test.

## Upstream and architecture

Forked from Veridian at `0c5776a4a4e08fd00b90d91ad3cd2ec10315d2bd`. Upstream Git
history and MIT attribution are retained; its original source and integration
files are archived in `vendor/veridian/`. The Tower LSP lifecycle/backend and
completion-helper patterns are adapted; its SV parser and external engines are
replaced with the suite's pinned AMS grammar and a scope-aware project index.
The grammar dependency is pinned to
`0b2672e76ff2941fb62081b5d42b0ae84a0f6c46` with Tree-sitter 0.26.13.

See [architecture](docs/ARCHITECTURE.md), [features/limits](docs/FEATURES.md), and
the original [Veridian project](https://github.com/vivekmalneedi/veridian).
