# Lil' Wrapper

Lil' Wrapper is a Rust library that hard-wraps comments and prose to a
configurable column. It is available as a Rust library, a native Language Server
Protocol (LSP) server, and a Zed extension.

## Documentation

| Component | Use it when you want to... |
| --- | --- |
| [Core library](crates/lil-wrapper-core/README.md) | Rewrap text from Rust code. |
| [LSP server](crates/lil-wrapper-lsp/README.md) | Integrate wrapping into an editor or LSP client. |
| [Zed extension](extension/README.md) | Configure and use Lil Wrapper in Zed. |

## Development

The workspace requires Rust 1.85 or newer. Run the core and LSP tests with:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The complete compatibility suite also requires the .NET 8 SDK and Deno 2. Building the Zed
extension requires the `wasm32-wasip2` target:

```sh
rustup target add wasm32-wasip2
cargo check --package lil-wrapper-zed --target wasm32-wasip2
```
