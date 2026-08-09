# Lil Wrapper CLI

A command line interface for the [Lil Wrapper core library](../lil-wrapper-core/README.md).
It wraps a file to a configured column from a terminal or script. Zed tasks use it to
surface Lil Wrapper operations in the command palette.

## Build

```sh
cargo build --release -p lil-wrapper-cli
```

The binary is written to `target/release/lil-wrapper-cli`.

## Usage

```sh
lil-wrapper-cli wrap <file> [--column N] [--tab-width N] [--write] [--help]
```

| Option | Description |
| --- | --- |
| `--column N` | Wrap at column N (default: 80). |
| `--tab-width N` | Tab expansion width (default: 4). |
| `--write` | Rewrite the file in place instead of printing to stdout. |
| `--help` | Print usage and exit. |

The wrap covers the whole file. Set `--column 0` to unwrap selected content; for the
CLI this unwraps the whole file. The language is detected from the file path, and the
wrap preserves comment markers, indentation, and leading whitespace.

## Example

```sh
lil-wrapper-cli wrap README.md --column 100 --write
```

## Zed tasks

Build the binary once (`cargo build -p lil-wrapper-cli`), then add a task so
`task: spawn` shows a Lil Wrapper entry for the current file. The task calls the
workspace binary directly, so no install or `PATH` entry is needed:

```json
[
  {
    "label": "Lil Wrapper: wrap file",
    "command": "target/debug/lil-wrapper-cli wrap --column 80 --write \"$ZED_FILE\"",
    "reveal": "never"
  }
]
```

See the [Zed extension README](../extension/README.md) for details.
