use cba::{ebog, ibog};
use std::{
    fs::{self},
    path::{Path, PathBuf},
    process::{Command, exit},
};

pub fn handle_download(cli: &crate::clap::Cli) {
    let Some(subfolder) = &cli.download else {
        return;
    };
    let presets_dir = crate::paths::presets_path();

    let temp_dir = std::env::temp_dir().join("mm_download");

    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).ok();
    }
    fs::create_dir_all(&temp_dir).unwrap();

    ibog!("Downloading presets from GitHub...");
    let zip_path = temp_dir.join("matchmaker.zip");

    let mut curl_cmd = Command::new("curl");
    curl_cmd.args([
        "-L",
        "https://github.com/Squirreljetpack/matchmaker/archive/refs/heads/main.zip",
        "-o",
    ]);
    curl_cmd.arg(&zip_path);

    let status = curl_cmd.status();
    if !status.is_ok_and(|x| x.success()) {
        ebog!("curl failed to download the presets.");
        exit(1);
    }

    ibog!("Extracting...");
    #[cfg(unix)]
    {
        let status = Command::new("unzip")
            .arg("-q")
            .arg(&zip_path)
            .current_dir(&temp_dir)
            .status();
        if !status.is_ok_and(|x| x.success()) {
            ebog!("unzip failed.");
            exit(1);
        }
    }
    #[cfg(windows)]
    {
        let status = Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}'",
                    zip_path.display(),
                    temp_dir.display()
                ),
            ])
            .status()
            .expect("Failed to execute powershell");
        if !status.success() {
            eprintln!("Error: powershell failed to extract the zip.");
            exit(1);
        }
    }

    let source_root = temp_dir.join("matchmaker-main/matchmaker-cli/assets/presets");
    let mut source = source_root.clone();
    let mut dest = presets_dir.to_path_buf();

    if !subfolder.is_empty() {
        let sub_path = source_root.join(subfolder);
        if sub_path.is_dir() {
            source = sub_path;
            dest = dest.join(subfolder);
        } else if subfolder.ends_with(".toml") {
            let path = Path::new(subfolder);
            let parent = path.parent().unwrap_or(Path::new(""));
            let file_name = path.file_name().unwrap().to_str().unwrap();

            let win_file_name = format!("win.{}", file_name);
            let win_path = source_root.join(parent).join(&win_file_name);
            let plain_path = source_root.join(subfolder);

            #[cfg(windows)]
            {
                if win_path.is_file() {
                    if !dest.exists() {
                        fs::create_dir_all(&dest).unwrap();
                    }
                    fs::copy(&win_path, dest.join(subfolder)).unwrap();
                    ibog!(
                        "Preset file successfully downloaded to: {}",
                        dest.join(subfolder).display()
                    );
                    fs::remove_dir_all(&temp_dir).ok();
                    exit(0);
                } else if plain_path.is_file() {
                    ebog!("Source '{}' is not available for your platform.", subfolder);
                    exit(1);
                } else {
                    ebog!("'{}' unavailable.", subfolder);
                    exit(1);
                }
            }
            #[cfg(not(windows))]
            {
                if plain_path.is_file() {
                    if !dest.exists() {
                        fs::create_dir_all(&dest).unwrap();
                    }
                    fs::copy(&plain_path, dest.join(subfolder)).unwrap();
                    ibog!(
                        "Preset file successfully downloaded to: {}",
                        dest.join(subfolder).display()
                    );
                    fs::remove_dir_all(&temp_dir).ok();
                    exit(0);
                } else if win_path.is_file() {
                    ebog!("Source '{}' is not available for your platform.", subfolder);
                    exit(1);
                } else {
                    ebog!("'{}' unavailable.", subfolder);
                    exit(1);
                }
            }
        } else {
            let suggested = if subfolder.ends_with(".toml") {
                let path = Path::new(subfolder);

                let parent = path.parent().unwrap_or(Path::new(""));

                let file_name = path.file_name().unwrap().to_str().unwrap();

                #[cfg(windows)]
                let candidate = parent.join(format!("win.{}", file_name));

                #[cfg(not(windows))]
                let candidate = parent.join(file_name);

                source_root
                    .join(&candidate)
                    .is_file()
                    .then(|| parent.join(file_name).display().to_string())
            } else {
                None
            };

            if let Some(suggested) = suggested {
                ebog!(
                    "'{}' not found in the repository. Did you mean '{}'?",
                    subfolder,
                    suggested
                );
            } else {
                ebog!("'{}' not found in the repository.", subfolder);
            }

            exit(1);
        }
    }

    let count = copy_and_process(&source, &dest);
    if count == 0 {
        ebog!("Source is not available for your platform.");
        exit(1);
    }

    ibog!("Presets successfully downloaded to: {}", dest.display());
    fs::remove_dir_all(&temp_dir).ok();
    exit(0);
}

fn copy_and_process(src: &Path, dst: &Path) -> usize {
    if !dst.exists() {
        fs::create_dir_all(dst).unwrap();
    }

    let mut count = 0;
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name_os = path.file_name().unwrap();
        let name = name_os.to_string_lossy();

        if path.is_dir() {
            count += copy_and_process(&path, &dst.join(name_os));
            continue;
        }

        #[cfg(windows)]
        {
            if let Some(stripped) = name.strip_prefix("win.") {
                fs::copy(&path, dst.join(stripped)).unwrap();
                count += 1;
            }
        }
        #[cfg(not(windows))]
        {
            if !name.starts_with("win.") {
                fs::copy(&path, dst.join(name.as_ref())).unwrap();
                count += 1;
            }
        }
    }
    count
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
