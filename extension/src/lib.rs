use std::fs;
use std::path::{Path, PathBuf};

use zed::settings::LspSettings;
use zed_extension_api as zed;

const BINARY_NAME: &str = "lil-wrapper-lsp";
const RELEASE_REPOSITORY: &str = "hypothesi/lil-wrapper";
const RELEASE_DIRECTORY_PREFIX: &str = "lil-wrapper-lsp-";

struct LilWrapperExtension {
    cached_binary: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlatformAsset {
    archive_name: &'static str,
    binary_name: &'static str,
    file_type: zed::DownloadedFileType,
}

impl LilWrapperExtension {
    fn resolve_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<PathBuf> {
        if let Some(path) =
            preferred_local_binary(worktree.which(BINARY_NAME), self.cached_binary.as_deref())
        {
            return Ok(path);
        }

        self.resolve_release_binary(language_server_id)
    }

    fn resolve_release_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
    ) -> zed::Result<PathBuf> {
        let (os, architecture) = zed::current_platform();
        let platform = platform_asset(os, architecture)?;
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let result = Self::download_latest(platform, language_server_id)
            .or_else(|release_error| {
                Self::find_installed_binary(platform.binary_name)
                    .map_err(|_| {
                        format!(
                            "Lil Wrapper language server is not on PATH and no downloaded release is available. Set lsp.lil-wrapper.binary.path for a local build. Release lookup failed: {release_error}"
                        )
                    })
            });

        let result = result.and_then(|path| {
            if os != zed::Os::Windows {
                zed::make_file_executable(path_string(&path)?)?;
            }
            self.cache(path)
        });

        match result {
            Ok(path) => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::None,
                );
                Ok(path)
            }
            Err(error) => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Failed(error.clone()),
                );
                Err(error)
            }
        }
    }

    fn download_latest(
        platform: PlatformAsset,
        language_server_id: &zed::LanguageServerId,
    ) -> zed::Result<PathBuf> {
        let release = zed::latest_github_release(
            RELEASE_REPOSITORY,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;
        let release_directory =
            PathBuf::from(format!("{RELEASE_DIRECTORY_PREFIX}{}", release.version));
        let binary_path = release_directory.join(platform.binary_name);
        if binary_path.is_file() {
            return Ok(binary_path);
        }

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == platform.archive_name)
            .ok_or_else(|| {
                format!(
                    "Lil Wrapper release {} has no {} asset",
                    release.version, platform.archive_name
                )
            })?;
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::Downloading,
        );
        let destination = path_string(&release_directory)?;
        zed::download_file(&asset.download_url, destination, platform.file_type)?;
        if !binary_path.is_file() {
            return Err(format!(
                "{} did not contain {} at its root",
                platform.archive_name, platform.binary_name
            ));
        }
        Ok(binary_path)
    }

    fn find_installed_binary(binary_name: &str) -> zed::Result<PathBuf> {
        find_installed_binary_in(Path::new("."), binary_name)
    }

    fn cache(&mut self, path: PathBuf) -> zed::Result<PathBuf> {
        if !path.is_file() {
            return Err(format!(
                "Lil Wrapper language server not found at {}",
                path.display()
            ));
        }
        self.cached_binary = Some(path.clone());
        Ok(path)
    }
}

fn find_installed_binary_in(directory: &Path, binary_name: &str) -> zed::Result<PathBuf> {
    let mut versions = fs::read_dir(directory)
        .map_err(|error| format!("failed to read the extension directory: {error}"))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version = entry
                .file_name()
                .to_str()?
                .strip_prefix(RELEASE_DIRECTORY_PREFIX)?
                .to_owned();
            let binary = entry.path().join(binary_name);
            binary.is_file().then_some((version, binary))
        })
        .collect::<Vec<_>>();
    versions.sort_by_key(|(version, _)| version_sort_key(version));
    versions
        .pop()
        .map(|(_, binary)| binary)
        .ok_or_else(|| "no downloaded Lil Wrapper language server is available".to_owned())
}

fn preferred_local_binary(
    path_binary: Option<String>,
    cached_binary: Option<&Path>,
) -> Option<PathBuf> {
    path_binary.map(PathBuf::from).or_else(|| {
        cached_binary
            .filter(|path| path.is_file())
            .map(Path::to_path_buf)
    })
}

impl zed::Extension for LilWrapperExtension {
    fn new() -> Self {
        Self {
            cached_binary: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings =
            LspSettings::for_worktree(language_server_id.as_ref(), worktree).unwrap_or_default();
        let configured_binary = settings.binary.as_ref();
        let command = configured_binary
            .and_then(|binary| binary.path.as_ref())
            .filter(|path| !path.trim().is_empty())
            .cloned()
            .map_or_else(
                || {
                    self.resolve_binary(language_server_id, worktree)
                        .and_then(|path| path_string(&path).map(ToOwned::to_owned))
                },
                Ok,
            )?;
        let args = configured_binary
            .and_then(|binary| binary.arguments.clone())
            .unwrap_or_default();
        let mut env = configured_binary
            .and_then(|binary| binary.env.clone())
            .map(|env| env.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        env.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        Ok(zed::Command { command, args, env })
    }

    fn language_server_initialization_options(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        Ok(
            LspSettings::for_worktree(language_server_id.as_ref(), worktree)?
                .initialization_options,
        )
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        Ok(LspSettings::for_worktree(language_server_id.as_ref(), worktree)?.settings)
    }
}

fn platform_asset(os: zed::Os, architecture: zed::Architecture) -> zed::Result<PlatformAsset> {
    use zed::{Architecture, DownloadedFileType, Os};

    match (os, architecture) {
        (Os::Mac, Architecture::Aarch64) => Ok(PlatformAsset {
            archive_name: "lil-wrapper-lsp-aarch64-apple-darwin.tar.gz",
            binary_name: BINARY_NAME,
            file_type: DownloadedFileType::GzipTar,
        }),
        (Os::Mac, Architecture::X8664) => Ok(PlatformAsset {
            archive_name: "lil-wrapper-lsp-x86_64-apple-darwin.tar.gz",
            binary_name: BINARY_NAME,
            file_type: DownloadedFileType::GzipTar,
        }),
        (Os::Linux, Architecture::Aarch64) => Ok(PlatformAsset {
            archive_name: "lil-wrapper-lsp-aarch64-unknown-linux-musl.tar.gz",
            binary_name: BINARY_NAME,
            file_type: DownloadedFileType::GzipTar,
        }),
        (Os::Linux, Architecture::X8664) => Ok(PlatformAsset {
            archive_name: "lil-wrapper-lsp-x86_64-unknown-linux-musl.tar.gz",
            binary_name: BINARY_NAME,
            file_type: DownloadedFileType::GzipTar,
        }),
        (Os::Windows, Architecture::Aarch64) => Ok(PlatformAsset {
            archive_name: "lil-wrapper-lsp-aarch64-pc-windows-msvc.zip",
            binary_name: "lil-wrapper-lsp.exe",
            file_type: DownloadedFileType::Zip,
        }),
        (Os::Windows, Architecture::X8664) => Ok(PlatformAsset {
            archive_name: "lil-wrapper-lsp-x86_64-pc-windows-msvc.zip",
            binary_name: "lil-wrapper-lsp.exe",
            file_type: DownloadedFileType::Zip,
        }),
        (_, Architecture::X86) => Err("32-bit x86 is not supported".to_owned()),
    }
}

fn path_string(path: &Path) -> zed::Result<&str> {
    path.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", path.display()))
}

fn version_sort_key(version: &str) -> Vec<u64> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

zed::register_extension!(LilWrapperExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_release_assets_for_supported_platforms() {
        let cases = [
            (
                zed::Os::Mac,
                zed::Architecture::Aarch64,
                "lil-wrapper-lsp-aarch64-apple-darwin.tar.gz",
                BINARY_NAME,
                zed::DownloadedFileType::GzipTar,
            ),
            (
                zed::Os::Mac,
                zed::Architecture::X8664,
                "lil-wrapper-lsp-x86_64-apple-darwin.tar.gz",
                BINARY_NAME,
                zed::DownloadedFileType::GzipTar,
            ),
            (
                zed::Os::Linux,
                zed::Architecture::Aarch64,
                "lil-wrapper-lsp-aarch64-unknown-linux-musl.tar.gz",
                BINARY_NAME,
                zed::DownloadedFileType::GzipTar,
            ),
            (
                zed::Os::Linux,
                zed::Architecture::X8664,
                "lil-wrapper-lsp-x86_64-unknown-linux-musl.tar.gz",
                BINARY_NAME,
                zed::DownloadedFileType::GzipTar,
            ),
            (
                zed::Os::Windows,
                zed::Architecture::Aarch64,
                "lil-wrapper-lsp-aarch64-pc-windows-msvc.zip",
                "lil-wrapper-lsp.exe",
                zed::DownloadedFileType::Zip,
            ),
            (
                zed::Os::Windows,
                zed::Architecture::X8664,
                "lil-wrapper-lsp-x86_64-pc-windows-msvc.zip",
                "lil-wrapper-lsp.exe",
                zed::DownloadedFileType::Zip,
            ),
        ];

        for (os, architecture, archive_name, executable, file_type) in cases {
            let asset = platform_asset(os, architecture).expect("supported platform");
            assert_eq!(asset.archive_name, archive_name);
            assert_eq!(asset.binary_name, executable);
            assert_eq!(asset.file_type, file_type);
        }
    }

    #[test]
    fn rejects_32_bit_x86() {
        for os in [zed::Os::Mac, zed::Os::Linux, zed::Os::Windows] {
            assert_eq!(
                platform_asset(os, zed::Architecture::X86),
                Err("32-bit x86 is not supported".to_owned())
            );
        }
    }

    #[test]
    fn orders_release_versions_numerically() {
        assert!(version_sort_key("v0.10.0") > version_sort_key("v0.9.9"));
        assert!(version_sort_key("v1.0.0") > version_sort_key("v0.99.99"));
    }

    #[test]
    fn finds_the_latest_downloaded_release_binary() {
        let root =
            std::env::temp_dir().join(format!("lil-wrapper-zed-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for version in ["v0.9.9", "v0.10.0"] {
            let directory = root.join(format!("{RELEASE_DIRECTORY_PREFIX}{version}"));
            fs::create_dir_all(&directory).expect("create cached release directory");
            fs::write(directory.join(BINARY_NAME), []).expect("create cached release binary");
        }

        let binary = find_installed_binary_in(&root, BINARY_NAME).expect("cached release binary");
        assert_eq!(
            binary,
            root.join(format!("{RELEASE_DIRECTORY_PREFIX}v0.10.0"))
                .join(BINARY_NAME)
        );
        fs::remove_dir_all(root).expect("remove cached release fixture");
    }

    #[test]
    fn prefers_worktree_path_and_ignores_stale_cached_binaries() {
        let root =
            std::env::temp_dir().join(format!("lil-wrapper-zed-path-test-{}", std::process::id()));
        let cached = root.join("cached-lil-wrapper-lsp");
        fs::create_dir_all(&root).expect("create local binary fixture");
        fs::write(&cached, []).expect("create cached binary fixture");

        assert_eq!(
            preferred_local_binary(
                Some("/worktree/bin/lil-wrapper-lsp".to_owned()),
                Some(&cached)
            ),
            Some(PathBuf::from("/worktree/bin/lil-wrapper-lsp"))
        );
        fs::remove_file(&cached).expect("make cached binary stale");
        assert_eq!(preferred_local_binary(None, Some(&cached)), None);
        fs::remove_dir_all(root).expect("remove local binary fixture");
    }
}
