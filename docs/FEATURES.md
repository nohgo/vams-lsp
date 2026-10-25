# Coverage and limitations

## Implemented

Module signatures come from the pinned AMS CST: ANSI/non-ANSI ports, grouped port
names, escaped names, module parameters/defaults, and connectmodule headers.
Body declarations retain header order. Incomplete signatures are withheld rather
than used to manufacture a whole instance. At a partially typed module-body line,
a transient whitespace probe can recover completion context without altering or
trusting the source buffer. A temporary `endmodule` in that probe also supports
new, unfinished modules. The source diagnostics stay intact; procedural/opaque
contexts and ambiguous recovery at the cursor remain excluded.

Standalone keyword completion can emit module, analog, begin/end, conditional,
loop, initial/always and generate templates. Snippet capability and context are
checked; clients without snippets and inline edits keep plain keyword items.
Actual Blink acceptance tests cover module/analog indentation and tabstops.

Scoped declarations include module nets/variables, ports, parameters/localparams,
analog functions and arguments, named blocks, branches, disciplines and natures.
Named connection labels resolve in the target module; expressions resolve in the
parent module. References are based on that resolution, not a global word search.
Duplicate global names return multiple definition targets and distinct completion
items; ambiguous named-connection checks are withheld.

Rename is deliberately narrower than references: non-macro, non-recovery,
non-library function-local variables/arguments/localparams only. Any existing
binding with the proposed name rejects the rename. It returns versioned document
edits and never performs a blind workspace-wide replacement.

Literal include navigation and bounded include-graph discovery work independently
of root source discovery. Directive selection supports line-leading `ifdef,
`ifndef, `elsif, `else, `endif, `define and `undef using project defines and
definitions in that file. Disabled source is masked with equal byte lengths, so
source locations remain in original files. File line endings/Unicode are retained.

## Deliberate boundaries

- This is not a modern SystemVerilog LSP or an elaborator. Packages, classes,
  interfaces and assertions are outside the grammar's target.
- There is no token macro expansion, token pasting/stringification, simulator
  file-list parsing, encrypted-source analysis, or full per-translation-unit macro
  environment for shared headers. Defines inside included files do not flow back
  into the including file's directive selection. Configure relevant defines
  explicitly and use Cadence for compiler-backed truth.
- Some macro-generated or aliased non-ANSI interfaces and inherited grammar edge
  cases cannot provide complete signatures. Unsupported syntax may be reported
  as parser recovery; this is not proof that Cadence would reject the file.
- No automatic net wiring, signal declaration insertion, parameter evaluation,
  generated hierarchy expansion, discipline/type compatibility checking, or
  convergence/hidden-state analysis is claimed.
- Global duplicate-name disambiguation is by visible source provenance; there is
  no simulator library mapping/elaboration model. Configure source/library sets
  to avoid unrelated definitions or choose among returned navigation targets.
- Document symbols include owned declaration scopes; unnamed procedural scopes
  are indexed for resolution but not all are rendered as outline containers.
- External files refresh every two seconds. Indexing is bounded and background;
  a large scan may temporarily show partial/older disk results. Open text always
  takes precedence. Changes exceeding the configured file-size limit are skipped
  and reported in project status.
- File contents are reparsed on accepted edits; LSP incremental text changes are
  supported, but Tree-sitter tree reuse and a persistent index database are not
  implemented. Configuration/status exposes progress and limits.
- No code actions, full hierarchy UI, semantic tokens, LSP formatting, or dynamic
  multi-root folder changes are advertised in this release.

A diagnostic-only pass recognizes missing module-header semicolons, including
balanced parameter/port lists and intervening comments. It reparses bounded header
insertions to avoid cascading highlights on subsequent declarations and maps all
remaining locations back to original UTF-8/UTF-16 source ranges. It skips ambiguous
macro/unbalanced headers. The original CST and raw error flags remain authoritative
for completion, navigation and rename safety. Isolated rejected commas/closing
delimiters are identified directly; parser MISSING tokens retain their names.

Syntax messages deliberately avoid claiming a specific cause when a recovery node
is broad. The fallback is "Incomplete or unrecognized Verilog-AMS syntax"; it
marks the affected syntax without guessing the root cause. Missing anonymous
tokens and ERROR nodes are both reported, with a 100-diagnostic cap. Existing formatter validation remains independent of the LSP.

## Measured smoke benchmark

On the development macOS arm64 machine, the release binary indexed a synthetic
1,001-file project in 0.385 seconds including server startup. Thirty warm module
completion requests had a 4.254 ms median and 4.998 ms p95, including stdio/JSON
round trips. Reproduce with `VAMS_LSP_BIN=target/release/vams-lsp python3
scripts/benchmark.py`. These tiny generated modules are not a proprietary model
library benchmark, and timing is not a cross-machine guarantee.
