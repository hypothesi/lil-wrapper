# Publishing Lil Wrapper to Zed

This document describes how to publish and update Lil Wrapper in the official Zed extension
registry. Zed distributes extensions from the
[`zed-industries/extensions`](https://github.com/zed-industries/extensions) repository; publishing
is a pull request to that repository, not an upload to a separate marketplace.

## One-Time Readiness Checks

Before the first submission, confirm all of the following:

- The public repository URL in `extension/extension.toml` is correct.
- The extension ID is `lil-wrapper`. It is unique as of 2026-08-08 and becomes permanent after
  publication. Hyphens are valid in Zed extension IDs; Zed's own publishing examples use
  `my-extension`. Cargo package names also support hyphens and expose them to Rust code with
  underscores, as used by `lil_wrapper_core` and `lil_wrapper_lsp`.
- `extension/extension.toml` contains all required manifest fields: `id`, `name`, `version`,
  `schema_version`, `authors`, `description`, and `repository`.
- `extension/LICENSE` is Apache-2.0. Zed accepts Apache-2.0 and requires the license in the
  registry entry's `path`, which is `extension`, rather than only at the repository root.
- The extension does not bundle the language server. `extension/src/lib.rs` locates a user-provided
  `lil-wrapper-lsp` first, then downloads an architecture-specific binary from this repository's
  GitHub release. This is the required model for language-server extensions.
- The extension has been installed and tested locally using `zed: install dev extension`. Test the
  wrapper actions and automatic release download on each platform that will be supported.
- Do not commit generated build output such as `extension/extension.wasm`; Zed packages the Rust
  extension from the source in `extension`.

Zed rejects IDs and names containing `zed`, `Zed`, or `extension`; `lil-wrapper` meets this rule.
It can also reject duplicate functionality, extensions that access the host environment outside the
Zed Extension API, or packages that include unrelated files.

## Publish a Release

The extension downloads native language-server assets, so make the GitHub release available before
submitting the registry pull request.

1. Choose the next semantic version, for example `0.1.0`.
2. Set that version in the workspace package metadata and in `extension/extension.toml`. The release
   workflow enforces that a `vX.Y.Z` tag matches every Cargo package and the extension manifest.
3. Run the local release checks:

   ```sh
   cargo test --locked --workspace
   cargo clippy --locked --workspace --all-targets -- -D warnings
   cargo fmt --all -- --check
   cargo check --locked --package lil-wrapper-zed --target wasm32-wasip2
   ```

4. Commit and push the version change, then create and push the matching tag. The tagged commit
   must be on the repository's default branch so the registry submodule does not point at a detached
   commit:

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

5. Wait for the repository's `Release` workflow to succeed. It publishes six assets, whose names
   must continue to match the names in `extension/src/lib.rs`:

   ```text
   lil-wrapper-lsp-aarch64-apple-darwin.tar.gz
   lil-wrapper-lsp-x86_64-apple-darwin.tar.gz
   lil-wrapper-lsp-aarch64-unknown-linux-musl.tar.gz
   lil-wrapper-lsp-x86_64-unknown-linux-musl.tar.gz
   lil-wrapper-lsp-aarch64-pc-windows-msvc.zip
   lil-wrapper-lsp-x86_64-pc-windows-msvc.zip
   ```

6. Test the tagged source as a Zed dev extension and verify that an installation without
   `lil-wrapper-lsp` on `PATH` downloads the matching asset. Use `zed: open log` or launch Zed with
   `zed --foreground` to diagnose extension output.

## Submit to the Registry

1. Fork `https://github.com/zed-industries/extensions` to a personal GitHub account. Zed recommends
   a personal fork so maintainers can apply necessary changes to the pull request.
2. Clone the fork and initialize its submodules:

   ```sh
   git clone https://github.com/<your-account>/extensions.git
   cd extensions
   git submodule init
   git submodule update
   ```

3. Add this public repository as an HTTPS submodule. The checked-out commit must be reachable from
   a branch, not detached. Confirm it is the released `vX.Y.Z` commit before continuing:

   ```sh
   git submodule add https://github.com/hypothesi/lil-wrapper.git extensions/lil-wrapper
   git -C extensions/lil-wrapper describe --exact-match --tags
   git add .gitmodules extensions/lil-wrapper
   ```

4. Add the following entry to the top-level `extensions.toml`:

   ```toml
   [lil-wrapper]
   submodule = "extensions/lil-wrapper"
   path = "extension"
   version = "X.Y.Z"
   ```

5. Sort the registry metadata:

   ```sh
   pnpm install
   pnpm sort-extensions
   ```

6. Commit the submodule pointer, `.gitmodules`, and `extensions.toml`, then open a pull request to
   `zed-industries/extensions`. State that Lil Wrapper is a native comment-wrapping language server,
   is tested as a dev extension, and downloads its platform-specific server release from the public
   repository.
7. Address registry CI or reviewer feedback. Once merged, Zed packages and publishes the extension
   automatically.

## Update a Published Extension

1. Complete the release procedure with a new version in `extension/extension.toml`.
2. Open a pull request to `zed-industries/extensions`.
3. Update its submodule pointer to the release commit:

   ```sh
   git submodule update --remote extensions/lil-wrapper
   ```

4. Change `[lil-wrapper].version` in the registry's `extensions.toml` to exactly match
   `extension/extension.toml` at that commit.
5. Run `pnpm sort-extensions`, commit the changes, and submit the update pull request.

## Sources

- [Zed: Developing Extensions](https://zed.dev/docs/extensions/developing-extensions)
- [Zed: Extension Publishing Prerequisites](https://zed.dev/docs/extensions/developing-extensions#extension-publishing-prerequisites)
- [Zed: Publishing Your Extension](https://zed.dev/docs/extensions/developing-extensions#publishing-your-extension)
- [Zed: Updating an Extension](https://zed.dev/docs/extensions/developing-extensions#updating-an-extension)
- [Zed Extensions registry contribution guidance](https://github.com/zed-industries/extensions/blob/main/CONTRIBUTING.md)
