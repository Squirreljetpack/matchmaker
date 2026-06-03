use cba::{ebog, ibog};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::{Command, exit},
};

const REPO: &str = "Squirreljetpack/matchmaker";
const BASE_PATH: &str = "matchmaker-cli/assets/presets";
const BRANCH: &str = "main";

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
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum GitHubResponse {
    Directory(Vec<GitHubFile>),
    File(GitHubFile),
    Error(GitHubError),
}

pub fn handle_download(cli: &crate::clap::Cli) {
    let Some(subfolder) = &cli.download else {
        return;
    };
    let presets_dir = crate::paths::presets_path();

    let mut remote_target = subfolder.clone();
    if cfg!(windows) && subfolder.ends_with(".toml") {
        let path = Path::new(subfolder);
        if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) {
            let win_name = format!("win.{}", file_name.to_string_lossy());
            remote_target = parent.join(win_name).to_string_lossy().into_owned();
        }
    }

    let api_url = format!(
        "https://api.github.com/repos/{}/contents/{}/{}?ref={}",
        REPO, BASE_PATH, remote_target, BRANCH
    );

    ibog!("Checking GitHub for '{}'...", subfolder);

    let output = Command::new("curl")
        .args(["-s", "-H", "User-Agent: matchmaker-cli", &api_url])
        .output()
        .expect("Failed to execute curl");

    // 1. Deserialize directly into our Untagged Enum
    let response: GitHubResponse = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        ebog!("Failed to parse GitHub response.");
        exit(1);
    });

    let items = match response {
        GitHubResponse::Directory(files) => files,
        GitHubResponse::File(file) => vec![file],
        GitHubResponse::Error(err) => {
            ebog!("GitHub API Error: {}", err.message);
            exit(1);
        }
    };

    let mut download_count = 0;

    for item in items {
        if item.entry_type != "file" {
            continue;
        }

        let download_url = match item.download_url {
            Some(url) => url,
            None => continue,
        };

        let is_toml = item.name.ends_with(".toml");
        let is_win_prefixed = item.name.starts_with("win.");

        let (should_download, local_name) = if is_toml {
            #[cfg(windows)]
            {
                if is_win_prefixed {
                    (true, item.name.strip_prefix("win.").unwrap().to_string())
                } else {
                    (false, String::new())
                }
            }
            #[cfg(not(windows))]
            {
                if !is_win_prefixed {
                    (true, item.name.clone())
                } else {
                    (false, String::new())
                }
            }
        } else {
            (true, item.name.clone())
        };

        if should_download {
            let dest_path = if subfolder.ends_with(".toml") {
                presets_dir.join(local_name)
            } else {
                presets_dir.join(subfolder).join(local_name)
            };

            if let Some(parent) = dest_path.parent() {
                if !cba::bs::create_dir(parent) {
                    std::process::exit(1)
                }
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

            if status.map_or(false, |s| s.success()) {
                download_count += 1;
            }
        }
    }

    if download_count > 0 {
        ibog!("Successfully downloaded {} file(s).", download_count);
    } else {
        ebog!("No compatible files found for your platform.");
        exit(1);
    }
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
        return "echo";
    }

    #[cfg(windows)]
    {
        "notepad"
    }
}
