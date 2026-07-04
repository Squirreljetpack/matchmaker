use cba::{
    _ibog,
    bog::{BogOkExt, BogUnwrapExt},
    broc::CommandExt,
    ebog, ibog,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::{Command, exit},
};

const REPO: &str = "Squirreljetpack/matchmaker";
const BASE_PATH: &str = "matchmaker-cli/assets/presets";
const BRANCH: &str = "main";

// todo: for base path, recurse into top-level directories

/// Build the GitHub `contents` API URL for a given path within the presets directory.
/// When `target` is empty, returns the URL for the presets root (no trailing slash),
/// since `…/presets/?ref=…` returns a 302 and yields an empty body that fails to parse.
fn build_api_url(target: &str) -> String {
    if target.is_empty() {
        format!("https://api.github.com/repos/{REPO}/contents/{BASE_PATH}?ref={BRANCH}")
    } else {
        format!("https://api.github.com/repos/{REPO}/contents/{BASE_PATH}/{target}?ref={BRANCH}")
    }
}

#[derive(Deserialize, Debug)]
pub struct GitHubFile {
    pub name: String,
    // "type" is a reserved keyword in Rust, so we remap it
    #[serde(rename = "type")]
    pub entry_type: String,
    pub download_url: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct GitHubError {
    pub message: String,
    pub status: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum GitHubResponse {
    Directory(Vec<GitHubFile>),
    File(GitHubFile),
    Error(GitHubError),
}

/// Handle `--download [FOLDER]`. `download` is the value of the flag: an
/// empty string downloads every preset, a folder path downloads that folder,
/// and a `.toml` file path downloads (and re-runs with `-o`) a file preset.
/// This function always exits — it either fetches what the user asked for
/// or errors out, and never returns to the caller.
pub fn handle_download(download: &String) -> ! {
    let subfolder = download;
    let presets_dir = crate::paths::presets_path();

    let is_unix = cfg!(target_os = "macos") || cfg!(target_os = "linux");
    let os_prefix = if cfg!(target_os = "windows") {
        "win."
    } else if cfg!(target_os = "macos") {
        "macos."
    } else if cfg!(target_os = "linux") {
        "linux."
    } else {
        ""
    };

    let mut candidates = Vec::new();
    if subfolder.ends_with(".toml") {
        let path = Path::new(subfolder);
        if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) {
            // 1. OS-specific
            if !os_prefix.is_empty() {
                let os_name = format!("{}{}", os_prefix, file_name.to_string_lossy());
                candidates.push(parent.join(os_name).to_string_lossy().into_owned());
            }
            // 2. Unix-specific
            if is_unix {
                let unix_name = format!("unix.{}", file_name.to_string_lossy());
                candidates.push(parent.join(unix_name).to_string_lossy().into_owned());
            }
            // 3. Generic
            candidates.push(subfolder.clone());
        } else {
            candidates.push(subfolder.clone());
        }
    } else {
        candidates.push(subfolder.clone());
    }

    let mut items = Vec::new();
    let mut found = false;

    for target in candidates {
        let api_url = build_api_url(&target);

        _ibog!("Checking GitHub for '{}'...", target);

        let output = Command::new("curl")
            .args(["-s", "-H", "User-Agent: matchmaker-cli", &api_url])
            .output()
            .expect("Failed to execute curl");

        let response: GitHubResponse =
            serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
                ebog!("Failed to parse GitHub response.");
                exit(1);
            });

        match response {
            GitHubResponse::Directory(files) => {
                items = files;
                found = true;
                break;
            }
            GitHubResponse::File(file) => {
                items = vec![file];
                found = true;
                break;
            }
            GitHubResponse::Error(err) if err.status == "404" => continue,
            GitHubResponse::Error(err) => {
                ebog!("GitHub API error: {} ({})", err.message, err.status);
                exit(1);
            }
        }
    }

    if !found {
        ebog!(
            "No compatible files found for '{}' on your platform.",
            subfolder
        );
        exit(1);
    }

    let mut download_count = 0;

    for item in items {
        if item.entry_type != "file" {
            continue;
        }

        let download_url = match item.download_url {
            Some(url) => url,
            None => continue,
        };

        let all_prefixes = ["win.", "macos.", "linux.", "unix."];

        let (mut skip, mut local_name) = (false, item.name.as_str());
        for p in all_prefixes {
            if let Some(name) = local_name.strip_prefix(p) {
                let is_compatible_unix = p == "unix." && is_unix;

                if p == os_prefix || is_compatible_unix {
                    local_name = name;
                } else {
                    skip = true;
                }
                break;
            }
        }

        if skip {
            continue;
        }

        let dest_path = if subfolder.ends_with(".toml") {
            presets_dir.join(local_name)
        } else {
            presets_dir.join(subfolder).join(local_name)
        };

        if let Some(parent) = dest_path.parent()
            && !cba::bs::create_dir(parent)
        {
            std::process::exit(1)
        }

        ibog!(
            "Downloading {}...",
            dest_path.file_name().unwrap().to_string_lossy()
        );

        let status = Command::new("curl")
            .args(["-L", "-s", "-o"])
            .arg(&dest_path)
            .arg(download_url)
            .status()
            .ok();

        if status.is_some_and(|s| s.success()) {
            download_count += 1;
        }
    }

    if download_count == 0 {
        ebog!("No compatible files found for your platform.");
        exit(1);
    }

    ibog!("Successfully downloaded {} file(s).", download_count);

    // `--download <file.toml>` follows up with mm -o.
    if subfolder.is_empty() || !subfolder.ends_with(".toml") {
        exit(0);
    } else {
        let file_name = Path::new(subfolder)
            .file_name()
            ._ebog("Unexpected: no filename")
            .to_string_lossy()
            .into_owned();
        let local_name = strip_platform_prefix(&file_name).unwrap_or(file_name);
        let exe = std::env::current_exe().__ebog();
        Command::new(exe)
            .with_arg("-o")
            .with_arg(local_name)
            ._exec();
    }
}

/// Strip a leading platform prefix (`win.`, `macos.`, `linux.`, `unix.`) from
/// `name`. Returns `None` if the prefix belongs to a different OS family
/// (e.g. `win.` on linux).
fn strip_platform_prefix(name: &str) -> Option<String> {
    const ALL_PREFIXES: &[&str] = &["win.", "macos.", "linux.", "unix."];
    for p in ALL_PREFIXES {
        if let Some(rest) = name.strip_prefix(p) {
            return Some(rest.to_string());
        }
    }
    Some(name.to_string())
}

pub fn expand_tilde(path: PathBuf) -> PathBuf {
    use std::path::Component;

    let mut components = path.components();

    match components.next() {
        Some(Component::Normal(first)) if first == "~" => {
            if let Some(home) = dirs::home_dir() {
                return home.join(components.as_path());
            }
        }

        _ => {}
    }

    path
}

#[allow(unused)]
pub fn guess_clip_cmd() -> Option<(String, String)> {
    #[cfg(target_os = "macos")]
    {
        if which::which("pbcopy").is_ok() && which::which("pbpaste").is_ok() {
            return Some(("pbcopy".to_string(), "pbpaste".to_string()));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if which::which("wl-copy").is_ok() {
            return Some(("wl-copy".to_string(), "wl-paste".to_string()));
        }

        if which::which("xclip").is_ok() {
            return Some((
                "xclip -selection clipboard -in".to_string(),
                "xclip -selection clipboard -out".to_string(),
            ));
        }

        if which::which("xsel").is_ok() {
            return Some((
                "xsel --clipboard --input".to_string(),
                "xsel --clipboard --output".to_string(),
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        return Some((
            "clip".to_string(),
            "powershell -command Get-Clipboard".to_string(),
        ));
    }

    None
}

pub fn guess_pager_cmd() -> &'static str {
    {
        for cmd in ["bat", "less", "more"] {
            if which::which(cmd).is_ok() {
                return cmd;
            }
        }
        "cat"
    }
}

pub fn guess_editor_cmd() -> &'static str {
    #[cfg(not(windows))]
    {
        for cmd in ["hx", "nvim", "vim", "vi", "nano"] {
            if which::which(cmd).is_ok() {
                return cmd;
            }
        }
        "echo"
    }

    #[cfg(windows)]
    {
        "notepad"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_api_url_empty_target_omits_trailing_slash() {
        // Regression: `mm --download` (no arg) used to produce
        // `…/presets/?ref=main`, which the GitHub API answers with a 302 and an
        // empty body, causing serde_json::from_slice to fail with the generic
        // "Failed to parse GitHub response." error.
        let url = build_api_url("");
        assert!(url.ends_with("?ref=main"), "url was {url}");
        assert!(
            !url.contains("/presets/?"),
            "url must not have an empty path segment: {url}"
        );
        assert!(url.contains("/presets?"), "url was {url}");
    }

    #[test]
    fn build_api_url_nonempty_target_keeps_slash() {
        let url = build_api_url("git");
        assert!(url.ends_with("/presets/git?ref=main"), "url was {url}");
    }

    #[test]
    fn build_api_url_nested_target_keeps_slash() {
        let url = build_api_url("git/grep.toml");
        assert!(
            url.ends_with("/presets/git/grep.toml?ref=main"),
            "url was {url}"
        );
    }
}
