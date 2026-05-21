use cba::{bog::BogOkExt, ebog, nbog};
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

    nbog!("Downloading presets from GitHub...");
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

    nbog!("Extracting...");
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

    let mut source = temp_dir.join("matchmaker-main/matchmaker-cli/assets/presets");
    let mut dest = presets_dir.to_path_buf();

    if !subfolder.is_empty() {
        #[allow(unused_mut)]
        let mut sub_path = source.join(subfolder);
        if sub_path.is_dir() {
            source = sub_path;
            dest = dest.join(subfolder);
        } else if subfolder.ends_with(".toml") && sub_path.is_file() {
            #[cfg(windows)]
            {
                sub_path = sub_path
                    .with_extension("")
                    .with_extension("")
                    .with_extension("win")
                    .with_extension("toml");
            }

            if !dest.exists() {
                fs::create_dir_all(&dest).unwrap();
            }
            let name = sub_path.file_name().unwrap();
            let copied = copy_single_file(&sub_path, &dest.join(name));

            if !copied {
                ebog!("Source '{}' is not available for your platform.", subfolder);
                exit(1);
            }

            let final_name = if cfg!(windows) {
                name.to_string_lossy().replace(".win.toml", ".toml")
            } else {
                name.to_string_lossy().into_owned()
            };

            nbog!(
                "Preset file successfully downloaded to: {}",
                dest.join(final_name).display()
            );
            fs::remove_dir_all(&temp_dir).ok();
            exit(0);
        } else {
            ebog!("'{}' not found in the repository.", subfolder);
            exit(1);
        }
    }

    copy_and_process(&source, &dest);

    nbog!("Presets successfully downloaded to: {}", dest.display());
    fs::remove_dir_all(&temp_dir).ok();
    exit(0);
}

fn copy_single_file(path: &Path, dest_path: &Path) -> bool {
    let name = path.file_name().unwrap().to_string_lossy();
    #[cfg(windows)]
    {
        if name_str.ends_with(".win.toml") {
            let new_name = name_str.replace(".win.toml", ".toml");
            fs::copy(path, dest_path.with_file_name(new_name)).__ebog();
            return true;
        } else if name_str.ends_with(".md") {
            fs::copy(path, dest_path).__ebog();
        }
        false
    }
    #[cfg(not(windows))]
    {
        if name.ends_with(".win.toml") {
            return false;
        }
        fs::copy(path, dest_path).__ebog();
        true
    }
}

fn copy_and_process(src: &Path, dst: &Path) {
    if !dst.exists() {
        fs::create_dir_all(dst).__ebog();
    }

    for entry in fs::read_dir(src).__ebog() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap();
        let dest_path = dst.join(name);

        if path.is_dir() {
            copy_and_process(&path, &dest_path);
        } else {
            copy_single_file(&path, &dest_path);
        }
    }
}

pub fn expand_tilde(path: PathBuf) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();

    if let Some(stripped) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
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
