# Offline diagnostic coverage

The server provides a practical syntax and local-consistency layer for both
traditional digital Verilog and analog AMS. It does not run a simulator or require
Cadence. The grammar owns syntax; the LSP adds explanations and scoped checks.

## Common checks

| Area | Examples of explanations |
| --- | --- |
| Statement/declaration terminators | Missing `;` after a port/net/variable/parameter/branch declaration, module instance, continuous/procedural assignment, analog contribution, function call, or nature/discipline attribute |
| Module and subroutine headers | Missing header `;`, including port lists and parameterized modules; function/task header recovery |
| Blocks | Missing/unexpected `end`, `endmodule`, `endcase`, `endfunction`, `endtask`, `endnature`, `enddiscipline`, `endgenerate`, and other supported paired constructs |
| Delimiters | Missing or mismatched parentheses, brackets and braces, with the expected closing token named |
| Expressions | Missing parameter initializers, assignment/contribution values, missing binary operands, empty `if`/`while` conditions, repeat counts, case selectors and event expressions |
| Analog mistakes | Using `=` on `V(...)`/`I(...)` instead of `<+`; nonblocking assignment in an analog block; contributions outside an analog block or inside a function |
| Text/directives | Unterminated comments, strings, macro argument lists and include filenames; malformed conditional directives |
| Language mismatch | Unambiguous SystemVerilog `always_ff`/`always_comb`/`always_latch` forms in an AMS file |
| Local declarations | Duplicate variables/constants, functions, branches, instances, or modules in a reliably recognized scope; a related location links to the first declaration |
| Named connections | Duplicate/unknown port and parameter labels when the target interface is unambiguous and sufficiently complete |

For example, `electrical p` followed by an analog statement is diagnosed as a
missing declaration semicolon, and `I(p,n) <+ ;` is diagnosed as a missing
expression after `<+`. A missing `end` before `endmodule` names both tokens
instead of underlining the whole module as an unexplained recovery region.

Nature and discipline **header** semicolons are optional in Verilog-AMS 2.4;
attribute semicolons are required. Half-open parameter ranges such as `[0:inf)`
are legal. `logic` may be a user-defined AMS discipline and is not automatically
rejected as a SystemVerilog type. These distinctions have regression controls.

## How explanations remain conservative

The lexer skips comments, quoted strings, escaped names, directive bodies and
opaque macro arguments. Conditional selection happens before analysis. Statement
semicolon suggestions must pass an isolated grammar probe: the construct must
fail without the terminator and parse cleanly with that one terminator. Work is
bounded by candidate count and statement length.

Explanations may use a temporary recovery parse to reduce cascades. Every position
is mapped back to the original source, including insertions/deletions and UTF-8,
UTF-16 and CRLF boundaries. Neither buffers nor the real analysis tree are edited.
Completion and rename retain the original error/scope safety checks; vamsfmt
retains independent syntax rejection and preservation checks.

Specific explanations take precedence over overlapping generic recovery cascades.
Unrelated errors remain visible. Editing a broken region does not globally turn
off checks on healthy instances/declarations elsewhere in the file. Stale versions
are still discarded, and Cadence publishes in its own independent namespace.

## Limits and future work

This is not full preprocessing, type checking or elaboration. Macro-generated
structures remain conservative. Incomplete interfaces, simulator library binding,
implicit nets and declarations brought into a module by includes can make a name
uncertain. The server therefore does not pretend to provide complete undefined-name,
port-width/direction, discipline compatibility, parameter evaluation, analog
operator placement/convergence, or driver/clocking analysis. Valid net/discipline
annotations and non-ANSI port redeclarations are not treated as duplicate variables.

Unsupported or ambiguous constructs still receive a generic syntax message.
Parser recovery can defer additional errors until the primary error is repaired.
Future checks should add malformed examples and nearby valid controls, not match
an error message to one particular pasted example.

## Verification

`tests/fixtures/diagnostics.json` contains 69 common-error/recovery scenarios and
23 valid neighboring controls.
`tests/diagnostics.rs` checks explanations, misleading-message exclusions, original
source preservation and encoding-safe ranges. Other Rust tests cover scoped
checks and incomplete-code completion; stdio tests check published messages and
versioned repair. Native Neovim tests check messages, repair, keyword/instance
acceptance and actual type capture priorities alongside Cadence coexistence.
