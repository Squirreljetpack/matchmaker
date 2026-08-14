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

/// The part of `name` after the *first* dot, e.g. `.gitignore` → `gitignore`
/// and `main.toml` → `toml`. Returns `None` when there is no dot. Unlike
/// [`Path::extension`], this matches dotfiles: `Path::new(".gitignore")` has
/// no `extension()` because its name begins with the dot.
fn extension_after_first_dot(name: &str) -> Option<&str> {
    name.split_once('.').map(|(_, after)| after)
}

/// The header that `raw.githubusercontent.com` serves for LFS-tracked files
/// instead of the real content.
const LFS_POINTER_HEADER: &str = "version https://git-lfs.github.com/spec/v1\n";

/// True when `content` is a Git LFS pointer file rather than real data.
fn is_lfs_pointer(content: &[u8]) -> bool {
    content.starts_with(LFS_POINTER_HEADER.as_bytes())
}

/// Map a `raw.githubusercontent.com` download URL to the
/// `media.githubusercontent.com/media/…` endpoint that serves the real Git LFS
/// content. URLs that are already media endpoints are returned unchanged.
fn media_url_from_raw(url: &str) -> String {
    const RAW_PREFIX: &str = "https://raw.githubusercontent.com/";
    match url.strip_prefix(RAW_PREFIX) {
        Some(rest) => format!("https://media.githubusercontent.com/media/{rest}"),
        None => url.to_string(),
    }
}

/// True when `path` is a script with a valid shebang on its first line:
/// `#!` followed (ignoring whitespace) by an absolute interpreter path, e.g.
/// `#!/bin/sh` or `#!/usr/bin/env python3`. The kernel rejects shebangs
/// without an absolute path, so a bare `#!` or relative interpreter is not
/// considered executable.
fn has_valid_shebang(path: &Path) -> bool {
    use std::io::Read;

    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };

    // Shebang lines are capped at 255 bytes by the kernel, so reading a
    // bounded first line keeps this cheap even for large files.
    let mut first_line = Vec::new();
    let mut byte = [0u8; 1];
    while first_line.len() < 256 {
        match file.read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => first_line.push(byte[0]),
            Err(_) => return false,
        }
    }

    match first_line.strip_prefix(b"#!") {
        Some(rest) => matches!(rest.iter().find(|b| !b.is_ascii_whitespace()), Some(&b'/')),
        None => false,
    }
}

/// True when `path` is a compiled binary in a known executable format:
/// ELF, Mach-O (32/64-bit, either endianness, and universal/fat), or PE.
/// Data files (zstd, png, …) never collide with these leading bytes.
fn has_executable_magic(path: &Path) -> bool {
    use std::io::Read;

    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut header = [0u8; 4];
    if file.read_exact(&mut header).is_err() {
        return false;
    }

    const EXECUTABLE_MAGICS: &[&[u8; 4]] = &[
        b"\x7fELF",
        b"\xce\xfa\xed\xfe", // Mach-O 32-bit LE
        b"\xcf\xfa\xed\xfe", // Mach-O 64-bit LE
        b"\xfe\xed\xfa\xce", // Mach-O 32-bit BE
        b"\xfe\xed\xfa\xcf", // Mach-O 64-bit BE
        b"\xca\xfe\xba\xbe", // universal/fat (also Java class files)
        b"\xca\xfe\xba\xbf", // universal/fat with 64-bit offsets
    ];
    EXECUTABLE_MAGICS.iter().any(|m| **m == header) || header.starts_with(b"MZ")
}

/// Add the executable bit to `path`. A no-op on platforms without POSIX
/// permission bits.
#[cfg(unix)]
fn set_executable_bit(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_mode(perms.mode() | 0o111);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn set_executable_bit(_path: &Path) {}

/// Handle `--download [FOLDER]`. `download` is the value of the flag: an
/// empty string downloads every preset, a folder path downloads that folder,
/// and a `.toml` file path downloads (and re-runs with `-o`) a file preset.
/// This function always exits — it either fetches what the user asked for
/// or errors out, and never returns to the caller.
pub fn handle_download(download: &String, folder_exclude_extensions: &[&str]) -> ! {
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

        // Skip files whose extension is in folder_exclude_extensions.
        // Matching on the part after the first dot also covers dotfiles like
        // `.gitignore`/`.gitattributes`, which `Path::extension()` reports as
        // having no extension.
        if let Some(ext) = extension_after_first_dot(&item.name)
            && folder_exclude_extensions.contains(&ext) {
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
            .arg(&download_url)
            .status()
            .ok();

        if status.is_some_and(|s| s.success()) {
            // `raw.githubusercontent.com` serves the LFS pointer text for
            // LFS-tracked files; re-fetch from the media endpoint to get the
            // real content.
            if std::fs::read(&dest_path).is_ok_and(|content| is_lfs_pointer(&content)) {
                ibog!(
                    "{} is a Git LFS pointer; fetching content via the media endpoint...",
                    local_name
                );
                let status = Command::new("curl")
                    .args(["-L", "-s", "-o"])
                    .arg(&dest_path)
                    .arg(media_url_from_raw(&download_url))
                    .status()
                    .ok();
                if !status.is_some_and(|s| s.success()) {
                    ebog!("Failed to fetch Git LFS content for '{}'.", local_name);
                    continue;
                }
            }

            // Scripts and compiled binaries (files the OS could execute
            // directly) are made executable after downloading.
            if has_valid_shebang(&dest_path) || has_executable_magic(&dest_path) {
                set_executable_bit(&dest_path);
            }
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

    #[test]
    fn extension_after_first_dot_matches_dotfiles() {
        assert_eq!(extension_after_first_dot(".gitignore"), Some("gitignore"));
        assert_eq!(
            extension_after_first_dot(".gitattributes"),
            Some("gitattributes")
        );
        assert_eq!(extension_after_first_dot("main.toml"), Some("toml"));
        assert_eq!(extension_after_first_dot("win.foo.tar.gz"), Some("foo.tar.gz"));
        assert_eq!(extension_after_first_dot("noext"), None);
    }

    #[test]
    fn is_lfs_pointer_detects_pointer_text() {
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 42\n";
        assert!(is_lfs_pointer(pointer));
        assert!(!is_lfs_pointer(b"\x28\xb5\x2f\xfd zstd magic"));
        assert!(!is_lfs_pointer(b"# Unicode & Emoji Picker\n"));
        assert!(!is_lfs_pointer(b""));
    }

    #[test]
    fn media_url_from_raw_rewrites_to_media_endpoint() {
        let raw = "https://raw.githubusercontent.com/Squirreljetpack/matchmaker/main/\
                   matchmaker-cli/assets/presets/unicode/unicode.zst";
        assert_eq!(
            media_url_from_raw(raw),
            "https://media.githubusercontent.com/media/Squirreljetpack/matchmaker/main/\
                   matchmaker-cli/assets/presets/unicode/unicode.zst"
        );
        let media = "https://media.githubusercontent.com/media/Squirreljetpack/matchmaker/main/x.y";
        assert_eq!(media_url_from_raw(media), media);
    }

    #[test]
    fn has_valid_shebang_accepts_only_absolute_interpreter_paths() {
        let dir = std::env::temp_dir().join("mm_shebang_test");
        std::fs::create_dir_all(&dir).unwrap();
        let cases: &[(&str, &[u8], bool)] = &[
            ("shell", b"#!/bin/sh\necho hi\n", true),
            ("env", b"#!/usr/bin/env python3\nprint(1)\n", true),
            ("spaced", b"#! /usr/bin/env bash\n", true),
            ("crlf", b"#!/bin/zsh\r\necho hi\r\n", true),
            ("bare", b"#!\n", false),
            ("whitespace", b"#!   \n", false),
            ("relative", b"#!python\n", false),
            ("plain_text", b"# not a shebang\n", false),
            ("binary", b"\x28\xb5\x2f\xfd zstd magic", false),
            ("empty", b"", false),
        ];
        for (name, content, expected) in cases {
            let p = dir.join(name);
            std::fs::write(&p, content).unwrap();
            assert_eq!(has_valid_shebang(&p), *expected, "case {name}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn has_executable_magic_recognizes_binary_formats() {
        let dir = std::env::temp_dir().join("mm_magic_test");
        std::fs::create_dir_all(&dir).unwrap();
        let cases: &[(&str, &[u8], bool)] = &[
            ("elf", b"\x7fELF\x02\x01\x01", true),
            ("macho64", b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01", true),
            ("macho32", b"\xce\xfa\xed\xfe", true),
            ("macho64be", b"\xfe\xed\xfa\xcf", true),
            ("macho32be", b"\xfe\xed\xfa\xce", true),
            ("fat", b"\xca\xfe\xba\xbe\x00\x00\x00\x02", true),
            ("fat64", b"\xca\xfe\xba\xbf", true),
            ("pe", b"MZ\x90\x00", true),
            ("zstd", b"\x28\xb5\x2f\xfd", false),
            ("png", b"\x89PNG\r\n", false),
            ("toml", b"# Unicode & Emoji Picker\n", false),
            ("short", b"\xcf\xfa", false),
            ("empty", b"", false),
        ];
        for (name, content, expected) in cases {
            let p = dir.join(name);
            std::fs::write(&p, content).unwrap();
            assert_eq!(has_executable_magic(&p), *expected, "case {name}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
