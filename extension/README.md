# Lil Wrapper for Zed

Lil Wrapper wraps comments and prose to the column you choose without leaving Zed. It is powered by
a native Rust language server and supports Markdown, plain text, and comments in a broad range of
programming and markup languages.

## Features

- Format an entire document or a selected range with Zed's normal formatting command.
- Rewrap comments and prose directly from the code-action menu.
- Choose from configured columns, unwrap selected content, or cycle through your rulers.
- Wrap complete comment blocks, not only the selected line.
- Preserve sentence spacing, indentation, and language-specific comment markers.
- Enable automatic wrapping as you type, per language and per document.
- Add custom line and block comment markers for languages outside the built-in set.
- Download a matching language-server release automatically when one is available; a local binary
  on your `PATH` or in settings takes precedence.

## Install

When Lil Wrapper is available in the Zed extension gallery, install it from Zed's Extensions panel.
The extension downloads the matching `lil-wrapper-lsp` release for supported macOS, Linux, and
Windows ARM64 and x86-64 systems.

To use the extension before a release is available, follow [Install from source](#install-from-source).

## Configure

Add an `lsp.lil-wrapper` entry to your Zed settings. This example wraps at column 80 and makes Lil
Wrapper the formatter for Markdown:

```json
{
  "lsp": {
    "lil-wrapper": {
      "settings": {
        "rewrap": {
          "wrappingColumn": 80,
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
  },
  "languages": {
    "Markdown": {
      "formatter": [
        {
          "language_server": {
            "name": "lil-wrapper"
          }
        }
      ]
    }
  }
}
```

### Choose A Column

Lil Wrapper resolves its column in this order:

1. A nonzero `rewrap.wrappingColumn` always wins.
2. When `wrappingColumn` is `0`, configured `editor.rulers` provide the available columns.
3. When no eligible ruler exists, `editor.wordWrapColumn` is used (80 by default).

Set `wrappingColumn` to `0` and provide several rulers to expose a code action for each one and to
cycle columns with repeated format commands:

```json
{
  "lsp": {
    "lil-wrapper": {
      "settings": {
        "rewrap": {
          "wrappingColumn": 0
        },
        "editor": {
          "rulers": [72, 80, 100],
          "wordWrapColumn": 80
        }
      }
    }
  }
}
```

### Turn On Auto-Wrap

Auto-wrap needs both Lil Wrapper's setting and Zed's on-type formatter setting for each language:

```json
{
  "lsp": {
    "lil-wrapper": {
      "settings": {
        "rewrap": {
          "autoWrap": {
            "enabled": true
          }
        }
      }
    }
  },
  "languages": {
    "Markdown": {
      "use_on_type_format": true
    }
  }
}
```

Use the `Toggle Auto-Wrap for Current Document` code action to override this setting temporarily
for one open document.

### Add Custom Comment Markers

For a language that does not have built-in comment syntax, define your delimiters under
`rewrap.customMarkers`:

```json
{
  "lsp": {
    "lil-wrapper": {
      "settings": {
        "rewrap": {
          "customMarkers": {
            "lineComment": "@@",
            "blockComment": ["<#", "#>"]
          }
        }
      }
    }
  }
}
```

Leave either marker empty when it does not apply.

## Use Lil Wrapper

1. Place the cursor in a comment or prose block, or select the content to change.
2. Run Zed's `editor: format` command to use the configured column.
3. Open the code-action menu for `Rewrap Comment / Text`, a direct `Rewrap at Column N` action,
   `Unwrap Comment / Text`, or the per-document auto-wrap toggle.

To bind `Alt+Q` to formatting, add this to your Zed keymap:

```json
[
  {
    "context": "Editor",
    "bindings": {
      "alt-q": "editor::Format"
    }
  }
]
```

Zed extensions cannot register an `Alt+Q` action directly, so this keybinding invokes Zed's normal
formatter command.

## Install From Source

Use a local build when developing Lil Wrapper or before a release binary is published:

```sh
cargo build --package lil-wrapper-lsp
```

In Zed, run `zed: install dev extension` and select this `extension` directory. Then point Zed at
the workspace binary:

```json
{
  "lsp": {
    "lil-wrapper": {
      "binary": {
        "path": "target/debug/lil-wrapper-lsp"
      }
    }
  }
}
```

The extension checks an explicit `binary.path` first, then `lil-wrapper-lsp` on the worktree
`PATH`, then its cached or downloaded release binary.

## Troubleshooting

| Problem | Check |
| --- | --- |
| No wrapping occurs with `editor: format`. | Configure Lil Wrapper as the formatter for the active language. |
| Auto-wrap does not run. | Enable both `rewrap.autoWrap.enabled` and the language's `use_on_type_format`. |
| Zed cannot start the server. | Build `lil-wrapper-lsp`, set `binary.path` to the local executable, or ensure it is on `PATH`. |
| A comment style is treated as plain text. | Set a supported language mode or configure `customMarkers`. |

For protocol-level integration details, see the [LSP server README](../crates/lil-wrapper-lsp/README.md).
For source, compatibility information, and release instructions, see the
[repository README](https://github.com/hypothesi/lil-wrapper).
