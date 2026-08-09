# lil-wrapper-core

`lil-wrapper-core` is the Rust library behind Lil Wrapper. Give it a document, editor-style
selections, and wrapping settings; it returns the replacement lines for the affected document
range. It recognizes comments and prose in supported markup and programming languages, while
plain text remains a useful fallback.

This crate is currently a workspace crate (`publish = false`). Depend on it by path from a project
in this repository, or vendor the crate into your workspace:

```toml
[dependencies]
lil-wrapper-core = { path = "../lil-wrapper/crates/lil-wrapper-core" }
```

## Quick Start

`wrap` works with newline-free lines and zero-based positions. Positions use UTF-16 code units,
matching the Language Server Protocol and most editor APIs.

```rust
use lil_wrapper_core::{
    wrap, CustomMarkers, File, Position, WrapRequest, Selection, Settings,
};

let request = WrapRequest {
    file: File {
        language: "plaintext".into(),
        path: "notes.txt".into(),
        custom_markers: CustomMarkers::default(),
    },
    settings: Settings {
        column: 12,
        tab_width: 4,
        double_sentence_spacing: false,
        reformat: false,
        whole_comment: true,
    },
    selections: vec![Selection {
        anchor: Position {
            line: 0,
            character: 0,
        },
        active: Position {
            line: 0,
            character: 18,
        },
    }],
    lines: vec!["one two three four".into()],
};

let edit = wrap(&request);

assert_eq!(edit.start_line, 0);
assert_eq!(edit.end_line, 0);
assert_eq!(edit.lines, ["one two", "three four"]);
```

Apply a non-empty result by replacing source lines from `start_line` through `end_line`, inclusive,
with `edit.lines`. An empty edit is a no-op: `Edit::is_empty()` is true and `end_line` is negative.
The `selections` on the returned edit represent the core's updated selections; an editor adapter can
use them after it applies the text replacement.

## Input Model

`WrapRequest` deliberately contains the context the wrapper needs:

| Field | Provide |
| --- | --- |
| `file.language` | A lower-case editor language ID such as `markdown`, `rust`, or `python`. |
| `file.path` | The file path or name. With empty or `plaintext` language IDs, it enables extension-based language detection. |
| `file.custom_markers` | Comment delimiters for an otherwise unsupported language. |
| `settings` | The target column and formatting behavior. |
| `selections` | One or more active ranges. An empty list lets the core process the whole document. |
| `lines` | Document lines without `\n` or `\r\n` terminators. |

`Selection` has an `anchor` and an `active` position, so it retains the direction of a selection.
Both fields are zero-based. Convert character offsets to UTF-16 code units before constructing a
`Position`; Rust byte indices and Unicode scalar counts are not interchangeable with this API.

## Settings

| Setting | Meaning |
| --- | --- |
| `column` | Target visual column. `0` removes wrapping within the selected block. |
| `tab_width` | Visual width of a tab stop. Use a positive value. |
| `double_sentence_spacing` | Preserve two spaces after sentence-ending punctuation when wrapping. |
| `reformat` | Reformat existing line breaks rather than preserving them where the language behavior allows it. |
| `whole_comment` | When a selection touches a comment, include the complete comment block. |

Visual width follows the reference behavior: tabs use `tab_width`, wide characters occupy two
columns, and positions remain UTF-16-based.

## Language Detection and Custom Markers

Use `language_name_for_file` to inspect the recognized language for a `File`, and `languages` to
list all supported names. Explicit, unsupported language IDs intentionally fall back to plain-text
wrapping. Empty and `plaintext` IDs can use the filename to detect formats such as Markdown,
Dockerfile, Rust, or Python.

For a language with nonstandard comments, supply markers directly:

```rust
use lil_wrapper_core::CustomMarkers;

let markers = CustomMarkers {
    line: "@@".into(),
    block: ("<#".into(), "#>".into()),
};
```

Use either marker type independently by leaving the unused string or tuple values empty.

## Auto-Wrap

`maybe_auto_wrap` supports editor on-type formatting. Pass the current request, the whitespace the
editor inserted, and the insertion position. It returns a no-op unless a single whitespace insertion
or newline crosses a positive wrapping column.

```rust
use lil_wrapper_core::{maybe_auto_wrap, Position};

let edit = maybe_auto_wrap(&request, " ", Position {
    line: 0,
    character: 18,
});
```

The LSP server manages document synchronization and converts this result to LSP edits. Prefer its
[LSP integration](../lil-wrapper-lsp/README.md) when you are implementing an editor client.

## Validate Changes

```sh
cargo test --package lil-wrapper-core
cargo clippy --package lil-wrapper-core --all-targets -- -D warnings
cargo fmt --all -- --check
```

The full workspace suite compares the Rust implementation against the pinned Rewrap reference.
See the repository [compatibility contract](../../COMPATIBILITY.md) for its prerequisites and scope.
