# lil-wrapper-lsp

`lil-wrapper-lsp` exposes Lil' Wrapper through the Language Server Protocol. It
runs over standard input and output, tracks open documents with incremental
UTF-16 synchronization, and provides formatting, range formatting, code actions,
commands, and optional on-type wrapping.

Use this server when an editor already has an LSP client, or when you are
writing one. For direct Rust use without JSON-RPC, see the [core
library](../lil-wrapper-core/README.md). For Zed, use the [Zed
extension](../../extension/README.md).

## Run The Server

Build the workspace binary with Rust 1.85 or newer:

```sh
cargo build --package lil-wrapper-lsp --release
./target/release/lil-wrapper-lsp
```

The process accepts and emits `Content-Length` framed JSON-RPC messages on stdio. Diagnostic output
is written to stderr. It takes no command-line protocol options.

An LSP client must send `initialize`, then `initialized`, before opening documents or requesting
formatting. Use normal LSP shutdown: request `shutdown`, wait for its `null` response, then notify
`exit`.

## Client Integration

Start `lil-wrapper-lsp` as a stdio server and advertise the capabilities that enable the features
you plan to use:

```json
{
  "capabilities": {
    "workspace": {
      "configuration": true,
      "applyEdit": true,
      "workspaceEdit": {
        "documentChanges": true
      }
    },
    "textDocument": {
      "codeAction": {
        "codeActionLiteralSupport": {
          "codeActionKind": {
            "valueSet": ["refactor.rewrite"]
          }
        }
      }
    }
  }
}
```

| Client capability | Enables |
| --- | --- |
| `workspace.configuration` | Per-workspace and per-document configuration requests. |
| `workspace.applyEdit` | The `rewrap.rewrapComment` and `rewrap.rewrapCommentAt` commands. |
| `workspace.workspaceEdit.documentChanges` | Versioned `documentChanges` edits; otherwise the server returns a `changes` map. |
| `textDocument.codeAction.codeActionLiteralSupport` for `refactor.rewrite` | Rewrap code actions. |

After `initialized`, reply to the server's `workspace/configuration` requests when configuration
support is advertised. Send `textDocument/didOpen` before formatting a document, keep it current
with monotonically increasing `didChange` versions, and send `didClose` when it closes. Document
positions, ranges, and edits use UTF-16 code units.

The server supports the following standard LSP features:

| Feature | Method | Notes |
| --- | --- | --- |
| Document formatting | `textDocument/formatting` | Wraps the open document. |
| Range formatting | `textDocument/rangeFormatting` | Wraps the selected range. |
| On-type formatting | `textDocument/onTypeFormatting` | Triggered by space, tab, and newline when auto-wrap is enabled. |
| Code actions | `textDocument/codeAction` | Returns `refactor.rewrite` actions. |

Formatting requests require `options.tabSize` and `options.insertSpaces`. A supplied `tabSize`
overrides configuration and must be a positive integer.

## Configure Wrapping

Supply configuration in `initialize.initializationOptions`,
`workspace/didChangeConfiguration`, or replies to `workspace/configuration`. The outer `settings`
object is optional. When replying to `workspace/configuration`, return the `rewrap` and `editor`
objects as the first and second values of the result array, respectively.

```json
{
  "settings": {
    "rewrap": {
      "wrappingColumn": 80,
      "tabWidth": 4,
      "doubleSentenceSpacing": false,
      "reformat": false,
      "wholeComment": true,
      "autoWrap": {
        "enabled": false
      },
      "customMarkers": {
        "lineComment": "",
        "blockComment": ["", ""]
      }
    },
    "editor": {
      "rulers": [80],
      "wordWrapColumn": 80,
      "tabSize": 4
    }
  }
}
```

| Setting | Default | Behavior |
| --- | --- | --- |
| `rewrap.wrappingColumn` | unset | A nonzero value is the sole wrapping column. Set `0` to use rulers or `wordWrapColumn`. |
| `editor.rulers` | `[]` | Numeric values or `{ "column": 80 }` values. They provide direct rewrap actions and are cycled by format commands. |
| `editor.wordWrapColumn` | `80` | Fallback column when no eligible ruler exists. |
| `editor.tabSize` | `4` | Preferred tab width. `rewrap.tabWidth` is used only when this is absent. |
| `rewrap.doubleSentenceSpacing` | `false` | Preserves sentence spacing during wrapping. |
| `rewrap.reformat` | `false` | Reformat existing breaks where supported. |
| `rewrap.wholeComment` | `true` | Wrap a complete comment block when touched by the selection. |
| `rewrap.autoWrap.enabled` | `false` | Enables on-type formatting. The legacy `autoWrapEnabled` key is also accepted. |
| `rewrap.customMarkers` | empty | Adds `lineComment` and/or `blockComment` markers for unsupported languages. |

`wrappingColumn` has priority over `rulers`, which have priority over `wordWrapColumn`. A resolved
column of `0` unwraps selected content. On consecutive successful format operations for the same
selection, the server rotates through configured rulers; a code action named `Rewrap Comment /
Text` uses the current column without advancing that cycle.

## Code Actions And Commands

When the client supports literal `refactor.rewrite` code actions, a code-action request returns:

- `Rewrap Comment / Text` at the current column.
- `Rewrap at Column N` for each distinct configured column.
- `Unwrap Comment / Text` at column `0`.
- `Toggle Auto-Wrap for Current Document`.

The server also advertises these commands:

| Command | Requirements | Result |
| --- | --- | --- |
| `rewrap.rewrapComment` | An open document, active range, and `workspace.applyEdit`. | Applies a wrap at the current or next ruler column. |
| `rewrap.rewrapCommentAt` | The same, plus a numeric column. | Applies a wrap at that column; `0` unwraps. |
| `rewrap.toggleAutoWrap` | A resolvable document URI. | Toggles auto-wrap only for that document and returns `{ "enabled": boolean }`. |

Use an object argument for custom commands:

```json
{
  "command": "rewrap.rewrapCommentAt",
  "arguments": [
    {
      "uri": "file:///workspace/notes.md",
      "range": {
        "start": { "line": 0, "character": 0 },
        "end": { "line": 2, "character": 10 }
      },
      "column": 80
    }
  ]
}
```

For the two rewrap commands, respond to the server's `workspace/applyEdit` request. The server uses
the `Rewrap Comment / Text` label and reports a command error if the client rejects the edit.

## Language IDs

Set `didOpen.textDocument.languageId` to a supported lower-case ID such as `markdown`, `rust`, or
`python` to enable syntax-aware wrapping. Use `plaintext` or an empty ID to let the server infer a
language from the file name. An explicit unrecognized ID deliberately uses plain-text behavior.

## Develop lil-wrapper-lsp

### Prerequisites

Developing the server requires Git and Rust 1.85 or newer. The focused LSP test suite has no .NET
or Deno requirement. The workspace compatibility suite additionally needs the .NET 8 SDK and Deno
2 because it checks the core against the pinned upstream references.

### Build And Run

```sh
cargo build --package lil-wrapper-lsp
cargo run --package lil-wrapper-lsp
```

Run it through an LSP client or a JSON-RPC harness; it communicates only through stdio. For a local
Zed integration, build this binary and follow the extension README's
[development installation](../../extension/README.md#install-from-source).

### Test And Lint

```sh
cargo test --package lil-wrapper-lsp
cargo clippy --package lil-wrapper-lsp --all-targets -- -D warnings
cargo fmt --all -- --check
```

Before changing wrapping behavior or release code, run the complete workspace check:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

See the repository [compatibility contract](../../COMPATIBILITY.md) for the reference corpus and
the [root README](../../README.md) for the other components.
