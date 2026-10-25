# Architecture

This is a focused fork of [Veridian](https://github.com/vivekmalneedi/veridian)
at `0c5776a4a4e08fd00b90d91ad3cd2ec10315d2bd`. Its Git history, MIT license,
and original implementation under `vendor/veridian/` are retained. The Tower LSP
backend/lifecycle and completion helper patterns are adapted from that server.
The SystemVerilog parser, external lint engines, and formatting ownership are
replaced with the suite's pinned AMS parser and offline project analysis.

`diagnostics` combines a shared source lexer, block/delimiter matching,
grammar-validated statement probes, expression checks and a presentation-only
recovery parse. Original-source position mapping and the untouched CST are kept
separate from explanations; the lexer also guards incomplete-code completion.
`syntax` extracts declarations, scopes, module signatures, occurrences, and
instances from Tree-sitter. `project` loads data-only configuration and discovers
bounded source/include graphs. `workspace` combines disk snapshots with versioned
open buffers. `features` resolves symbols and creates LSP results. `server` adapts
Veridian's protocol structure, with background refresh and revision-safe updates.

Each Neovim client represents one project root. Open buffers override disk text.
Project scans run off the protocol threads and merge without overwriting newer
open documents. Feature requests use cached snapshots. Config changes preserve
the last good configuration on failure. Syntax diagnostics belong to the server;
grammar highlighting, vamsfmt formatting, and Cadence checks retain their owners.

No full simulator elaboration is attempted. Supported directives select literal
conditional branches; arbitrary macro expansion and translation-unit-dependent
header semantics remain explicitly limited. Ambiguous module definitions produce
multiple navigation results, and incomplete interfaces do not produce instance
snippets. Rename is restricted to reliably bound, non-macro local symbols.
