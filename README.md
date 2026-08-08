# Rewrap for Zed

Rewrap wraps comments and prose to a configurable column. This port independently implements
Rewrap 1.16.3 in Rust and runs through a native language server. The published extension does not
compile, invoke, or ship the original F# or JavaScript runtime. A test-only compatibility host
compiles the pinned F# core to compare it directly with Rust.

## Prerequisites

Building the language server requires Git and Rust 1.85 or newer. Building the Zed WebAssembly
extension also requires the `wasm32-wasip2` Rust target:

```sh
rustup target add wasm32-wasip2
```

The compatibility suite additionally requires the .NET 8 SDK and Deno 2. The first test run needs
network access to populate the Cargo, NuGet, and Deno caches. Production builds and release packages
do not use .NET, Deno, or JavaScript.

Manual editor verification requires Zed 1.14.2 or newer. The recorded fixture is in
`tests/zed-manual`.

## Development installation

Build the native language server before installing the development extension:

```sh
cargo build --package rewrap-lsp
```

Run `zed: install dev extension` in Zed and select the `extension` directory. The extension accepts
`rewrap-lsp` from the worktree shell `PATH`.

An explicit binary path can be set in Zed settings:

```json
{
  "lsp": {
    "rewrap": {
      "binary": {
        "path": "target/debug/rewrap-lsp"
      }
    }
  }
}
```

Release packages provide matching `rewrap-lsp` archives. Each archive contains `rewrap-lsp`, or
`rewrap-lsp.exe` on Windows, plus `LICENSE` and `THIRD_PARTY_NOTICES.md` at its root. Until the first
release is published, development installations must use the workspace binary setting or put
`rewrap-lsp` on `PATH`.

## Settings

Pass Rewrap settings through the `rewrap` language server entry:

```json
{
  "lsp": {
    "rewrap": {
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
  }
}
```

`wrappingColumn` takes precedence over rulers and `wordWrapColumn`. Set it to `0` to cycle through
configured rulers.

Auto-wrap also requires Zed's on-type formatter setting for each applicable language:

```json
{
  "languages": {
    "Markdown": {
      "use_on_type_format": true
    }
  }
}
```

## Commands and keybindings

Rewrap appears in the editor code-action menu with actions for the active column, each configured
column, unwrapping, and per-document auto-wrap toggling. To use `editor: format`, select Rewrap as
the formatter globally or for individual languages:

```json
{
  "languages": {
    "Markdown": {
      "formatter": [
        {
          "language_server": {
            "name": "rewrap"
          }
        }
      ]
    }
  }
}
```

Zed extensions cannot register an `Alt+Q` action. Add an editor keybinding for
`editor::Format` if that shortcut is preferred:

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

Zed also has no extension API for a free-text column prompt, status-bar notifications, or direct
selection restoration. Configured columns are direct code actions, arbitrary columns require the
`rewrap.rewrapCommentAt` LSP command with a numeric argument, and selection updates remain owned by
the editor.

## Verification

`cargo test --workspace` runs the unchanged upstream F# runner, the direct F#/Rust oracle, and the
exact pinned TypeScript adapter through Deno. It does not install Node packages and requires
`node_modules` to remain absent.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check --package rewrap-zed --target wasm32-wasip2
```

The release workflow uses Rust 1.88.0 to build and inspect archives for macOS, Linux, and Windows on
ARM64 and x86-64. Pushing a `v*` tag creates a GitHub release only after all six archives pass their
root-content checks. A manual workflow run builds the same archives without publishing a release.

The publishable Zed extension is isolated in `extension`. A Zed registry entry for this repository
must set its `path` field to `extension`; the language server, vendored reference, and test-only
oracles are outside that package boundary.

The vendored source can also be checked against an independent Rewrap checkout:

```sh
REWRAP_REFERENCE_ROOT=/path/to/Rewrap \
  cargo test --package rewrap-core pinned_reference_files_match_the_verified_upstream_tree
```

See `COMPATIBILITY.md` for the pinned reference corpus and documented host differences. See
`ZED_MANUAL_TEST.md` for the Zed 1.14.2 editor test matrix.
