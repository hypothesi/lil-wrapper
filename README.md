# Lil' Wrapper

Lil' Wrapper is a Rust library that hard-wraps comments and prose to a
configurable column. It is available as a Rust library, a native Language Server
Protocol (LSP) server, and a Zed extension.

## Documentation

| Component | Use it when you want to... |
| --- | --- |
| [Core library](crates/lil-wrapper-core/README.md) | Wrap text from Rust code. |
| [LSP server](crates/lil-wrapper-lsp/README.md) | Integrate wrapping into an editor or LSP client. |
| [Zed extension](extension/README.md) | Configure and use Lil Wrapper in Zed. |
| [CLI](crates/lil-wrapper-cli/README.md) | Wrap a file from a terminal or script. |

## Development

The workspace requires Rust 1.85 or newer. Run the core and LSP tests with:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The compatibility suite compares this implementation against the original Rewrap, so
`cargo test --workspace` needs the upstream sources in `vendor/rewrap`. They are not committed
here. Restore them once, at the pinned commit that the suite verifies byte-for-byte:

```sh
git clone --filter=blob:none --no-checkout https://github.com/stkb/Rewrap.git vendor/rewrap
git -C vendor/rewrap checkout --detach 6ba6e3db36686f713e0180f1a5bbefcc9685e144
```

That suite also requires the .NET 8 SDK and Deno 2. Building the Zed
extension requires the `wasm32-wasip2` target:

```sh
rustup target add wasm32-wasip2
cargo check --package lil-wrapper-zed --target wasm32-wasip2
```
