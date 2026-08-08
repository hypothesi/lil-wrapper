# Compatibility contract

The Rewrap v1.16.3 F# and TypeScript sources are pinned read-only references. The published Rust
product does not compile, link, invoke, or ship the original runtime. The compatibility suite does
compile the pinned F# core through a test-only .NET 8 host so that it can compare both
implementations directly.

## Automated coverage

| Reference behavior | Rust contract |
| --- | --- |
| Pinned F#, TypeScript, manifest, license, and specification inventory and bytes | `pinned_reference_files_match_the_verified_upstream_tree` in `crates/rewrap-core/tests/reference_differential.rs` |
| Original F# runner passes all 470 executable expectations | `crates/rewrap-core/tests/reference_differential.rs` |
| Rust passes the same 470 executable expectations | `crates/rewrap-core/tests/spec_compatibility.rs` |
| Original F# and Rust parsers produce identical records for all 482 cases | `crates/rewrap-core/tests/reference_differential.rs` |
| F# and Rust return equivalent edits for all 482 parsed cases | `crates/rewrap-core/tests/reference_differential.rs` |
| Wrapping, auto-wrap, width, language, marker, ruler, and every source-comment marker family match F# | `crates/rewrap-core/tests/reference_differential.rs` |
| Spec grammar, settings, selections, tabs, and corpus inventory | `crates/rewrap-core/tests/spec_harness.rs` |
| Additional uncovered core behavior and safety regressions | `crates/rewrap-core/tests/reference_regressions.rs` |
| UTF-16 visual width and language lookup | `crates/rewrap-core/tests/core_contract.rs` |
| Ruler state and consecutive-wrap cycling | `crates/rewrap-core/tests/core_contract.rs` |
| Auto-wrap eligibility and cursor result | `crates/rewrap-core/tests/core_contract.rs` |
| Custom line and block comment markers | `crates/rewrap-core/tests/core_contract.rs` |
| Wrapping-column precedence and setting validation | `crates/rewrap-lsp/tests/settings_contract.rs` |
| Selection remapping for versioned, editor-owned edits | `crates/rewrap-lsp/tests/selection_contract.rs` |
| LSP lifecycle and advertised capabilities | `crates/rewrap-lsp/tests/protocol_contract.rs` |
| Incremental UTF-16 synchronization and versioned edits | `crates/rewrap-lsp/tests/protocol_contract.rs` |
| CRLF preservation and runtime configuration | `crates/rewrap-lsp/tests/protocol_contract.rs` |
| Wrap, custom-column, unwrap, and per-document auto-wrap actions | `crates/rewrap-lsp/tests/protocol_contract.rs` |
| Exact VS Code manifest commands and adapter-to-core command requests | `crates/rewrap-lsp/tests/vscode_adapter_differential.rs` |
| Standard wrap, custom-column, unwrap, cancellation, range, edit, and failed-edit command paths | `crates/rewrap-lsp/tests/vscode_adapter_differential.rs` |
| VS Code settings, custom-language cache, and `fast-diff` selection remapping | `crates/rewrap-lsp/tests/vscode_adapter_differential.rs` |
| VS Code auto-wrap event gates, configuration transitions, and document toggle state | `crates/rewrap-lsp/tests/vscode_adapter_differential.rs` |
| macOS, Linux, and Windows release asset filename mapping | `extension/src/lib.rs` unit tests |

The corpus contains 482 parsed cases across 58 Markdown files: 474 primary cases and 8 `-or-`
alternatives. The original runner skips every case with `reformat = true`, including 4 primary
cases and all 8 alternatives, so its executable suite contains 470 expectations.

The differential oracle invokes the original core for all 482 cases, including the 12 skipped by
the original runner, and compares complete edit text and selections. Equivalent empty-edit
sentinels are canonicalized because the public F# contract explicitly defines any empty edit with
`endLine < startLine` as a no-op. Two skipped Markdown prose expectations disagree with the actual
F# runtime; Rust follows the runtime, and focused regressions preserve those observed results.

## Intentional post-reference fixes

After the automated compatibility gate passed, three severe upstream defects were selected from the
open issue backlog. Rust intentionally differs from v1.16.3 only for these focused cases:

| Upstream issue | Rust behavior | Regression |
| --- | --- | --- |
| [#403](https://github.com/stkb/Rewrap/issues/403) | Protected spaces in Javadoc inline tags are restored without leaking U+0000; literal U+0000 content remains unchanged. | `javadoc_inline_tags_never_leak_protected_space_sentinels` |
| [#419](https://github.com/stkb/Rewrap/issues/419), [#418](https://github.com/stkb/Rewrap/issues/418), [#344](https://github.com/stkb/Rewrap/issues/344), and [#310](https://github.com/stkb/Rewrap/issues/310) | Python docstring continuation lines do not repeat plain or prefixed triple-quote opening delimiters. | `python_docstring_continuations_do_not_repeat_the_opening_delimiter` and `prefixed_python_docstring_continuations_do_not_repeat_the_opening_delimiter` |
| [#258](https://github.com/stkb/Rewrap/issues/258) | C# XML documentation start tags remain intact, including spaces after quoted `>` characters. | `xmldoc_start_tags_are_kept_intact_when_wrapping` |

The pinned F# and TypeScript oracles remain unchanged. The complete reference corpus and adapter
contracts still pass, while these regressions document the deliberate behavior improvements.

## Zed extension boundary

The manifest attaches Rewrap to 77 exact Zed 1.14.2 language names representing 63 reference
languages and their real Zed variants. Eleven reference languages have no registered Zed language;
when Zed opens those files as Plain Text, Rewrap can still infer supported file suffixes.

The Zed host boundary is verified separately and does not count as automated compatibility proof.
Zed 1.14.2 compiled the final Rust extension to WebAssembly and loaded it as a development
extension. It started `target/debug/rewrap-lsp` through the workspace binary setting. Document,
range, and multiple-selection formatting passed. Undo, unwrap, auto-wrap, and the per-document
toggle also passed. `ZED_MANUAL_TEST.md` contains the complete editor test record.

## Reference-only behavior

The following VS Code UI details have no exact Zed extension API equivalent and are verified at
the closest supported boundary:

| VS Code behavior | Zed-compatible verification |
| --- | --- |
| `Alt+Q` command registration | Manual keymap plus LSP formatting/code-action execution |
| Custom-column input box | Direct actions for configured columns; arbitrary columns require a numeric `workspace/executeCommand` argument |
| Status-bar auto-wrap notification | Per-document toggle state and on-type formatting behavior |
| Direct active-editor selection restoration | Versioned LSP edits plus editor-owned selection behavior |

These are host API differences, not omissions from the Rust wrapping implementation.

Zed must also have `use_on_type_format` enabled for a language before it sends auto-wrap requests.
This is a native Zed language setting and is separate from Rewrap's `autoWrap.enabled` setting.
