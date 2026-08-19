//! Command execution strategy classification, shared by every execution path
//! (Execute payloads, dynamic `[envs]`, `start.directory`, Transform, Copy).

use std::{ffi::OsString, path::Path};

use cba::{bring::split::split_whitespace_preserve_single_quotes, broc::EnvVars};
use matchmaker::{config_mm::ConfigPreprocessedData, render::MMState};

/// How a script string is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandStrategy {
    /// Plain shell command (or templated shell command string, runs via `$SHELL -c`).
    Shell(String),
    /// `@path [args…]` — direct execution without shell interpretation.
    File { path: OsString, args: Vec<OsString> },
    /// `#!lua …` — inline lua source for the lua engine.
    #[cfg(feature = "mlua")]
    Lua(String),
    /// `@file.lua [args…]` — lua script file; `args` are the script's varargs.
    #[cfg(feature = "mlua")]
    LuaFile { path: OsString, args: Vec<OsString> },
}

impl CommandStrategy {
    /// Whether this strategy executes in the Lua runtime.
    pub fn is_lua(&self) -> bool {
        match self {
            #[cfg(feature = "mlua")]
            CommandStrategy::Lua(_) | CommandStrategy::LuaFile { .. } => true,
            _ => false,
        }
    }

    /// If this is a [`CommandStrategy::Shell`], perform template interpolation with `state`.
    /// Returns `None` if formatting produced an empty string (signaling no action / missing selection).
    pub fn template(
        self,
        state: &MMState<'_, String, ConfigPreprocessedData>,
    ) -> Option<CommandStrategy> {
        match self {
            CommandStrategy::Shell(cmd) => {
                let formatted = crate::formatter::format_cli(state, &cmd, None);
                if formatted.is_empty() {
                    None
                } else {
                    Some(CommandStrategy::Shell(formatted))
                }
            }
            other => Some(other),
        }
    }

    /// Resolve relative file paths for [`CommandStrategy::File`] and [`CommandStrategy::LuaFile`]
    /// against the parent directory of `MM_OVERRIDE`.
    pub fn resolve_relative(mut self, envs: &EnvVars) -> Result<CommandStrategy, String> {
        let path = match &mut self {
            CommandStrategy::File { path, .. } => path,
            #[cfg(feature = "mlua")]
            CommandStrategy::LuaFile { path, .. } => path,
            _ => return Ok(self),
        };

        if !Path::new(&path).is_absolute() {
            let Some(override_path) = envs.get("MM_OVERRIDE") else {
                return Err(format!(
                    "MM_OVERRIDE not set; skipping @ command: {}",
                    path.to_string_lossy()
                ));
            };
            *path = Path::new(override_path)
                .parent()
                .unwrap_or(Path::new(""))
                .join(&path)
                .into_os_string();
        }

        Ok(self)
    }
}

/// Classify a script string.
///
/// `#!lua` and `@*.lua` payloads are only recognized when the `mlua` feature
/// is enabled; otherwise they fall back to [`CommandStrategy::Shell`] and
/// [`CommandStrategy::File`].
#[cfg_attr(not(feature = "mlua"), allow(unused_variables))]
pub fn classify(s: &str) -> CommandStrategy {
    #[cfg(feature = "mlua")]
    if let Some(rest) = s.strip_prefix("#!lua")
        && rest.starts_with(char::is_whitespace)
    {
        return CommandStrategy::Lua(rest.trim_start().to_owned());
    }

    let Some(rest) = s.strip_prefix('@') else {
        return CommandStrategy::Shell(s.to_owned());
    };
    let mut words = split_whitespace_preserve_single_quotes(rest).into_iter();
    let Some(path) = words.next() else {
        return CommandStrategy::Shell(s.to_owned());
    };
    let args: Vec<OsString> = words.map(OsString::from).collect();
    #[cfg(feature = "mlua")]
    if Path::new(&path).extension().is_some_and(|e| e == "lua") {
        return CommandStrategy::LuaFile {
            path: path.into(),
            args,
        };
    }
    CommandStrategy::File {
        path: path.into(),
        args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cba::env_vars;

    #[cfg(feature = "mlua")]
    #[test]
    fn lua_prefix_accepts_any_whitespace() {
        for s in [
            "#!lua return 1",
            "#!lua  return 1",
            "#!lua\nreturn 1",
            "#!lua\r\nreturn 1",
            "#!lua\treturn 1",
        ] {
            assert!(
                matches!(classify(s), CommandStrategy::Lua(code) if code == "return 1"),
                "expected Lua for {s:?}"
            );
        }
        // a bare `#!lua` with no separator is not a lua marker
        assert!(
            matches!(classify("#!luaprint('x')"), CommandStrategy::Shell(s) if s == "#!luaprint('x')")
        );
    }

    #[cfg(feature = "mlua")]
    #[test]
    fn classify_recognizes_lua_files() {
        assert!(matches!(
            classify("@clean.lua 'a b' c"),
            CommandStrategy::LuaFile { path, args } if path == "clean.lua" && args == ["a b", "c"]
        ));
    }

    #[test]
    fn classify_recognizes_file_and_shell() {
        assert!(matches!(
            classify("@script.sh 'a b' c"),
            CommandStrategy::File { path, args } if path == "script.sh" && args == ["a b", "c"]
        ));
        assert!(matches!(
            classify("echo hi"),
            CommandStrategy::Shell(s) if s == "echo hi"
        ));
    }

    #[tokio::test]
    async fn test_template() {
        use crate::formatter::tests::{offline_ui, push_items, setup_test_mm};
        use matchmaker::render::State;
        use matchmaker::ui::DisplayUI;
        use tokio::sync::mpsc;

        let (mut mm, injector, _guard) = setup_test_mm();
        push_items(&mut mm, injector, &["a,b,c"], 1);
        let mut state_obj = State::new();
        let (mut ui, mut picker_ui) = offline_ui(mm);
        let mut footer_ui = DisplayUI::default();
        let mut preview_ui = None;
        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        let mm_state = state_obj.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &event_tx,
        );

        assert_eq!(
            classify("echo {col1}").template(&mm_state),
            Some(CommandStrategy::Shell("echo 'a'".to_string()))
        );
        assert_eq!(
            classify("@script.sh a b").template(&mm_state),
            Some(CommandStrategy::File {
                path: OsString::from("script.sh"),
                args: vec![OsString::from("a"), OsString::from("b")],
            })
        );
    }

    #[test]
    fn test_resolve_relative() {
        let envs = env_vars!("MM_OVERRIDE" => "/path/to/preset.toml");
        let empty_envs = EnvVars::new();

        // Relative File strategy with MM_OVERRIDE
        let file_strat = classify("@script.sh a b");
        assert_eq!(
            file_strat.clone().resolve_relative(&envs).unwrap(),
            CommandStrategy::File {
                path: Path::new("/path/to/script.sh").as_os_str().to_os_string(),
                args: vec![OsString::from("a"), OsString::from("b")],
            }
        );

        // Relative File strategy without MM_OVERRIDE fails
        assert!(file_strat.resolve_relative(&empty_envs).is_err());

        // Absolute File strategy succeeds without MM_OVERRIDE
        let abs_strat = classify("@/usr/local/bin/script.sh a");
        assert_eq!(
            abs_strat.clone().resolve_relative(&empty_envs).unwrap(),
            CommandStrategy::File {
                path: OsString::from("/usr/local/bin/script.sh"),
                args: vec![OsString::from("a")],
            }
        );

        // Shell strategy succeeds regardless of MM_OVERRIDE
        let shell_strat = classify("echo hi");
        assert_eq!(
            shell_strat.clone().resolve_relative(&empty_envs).unwrap(),
            CommandStrategy::Shell("echo hi".to_string())
        );

        #[cfg(feature = "mlua")]
        {
            let lua_file_strat = classify("@clean.lua a");
            assert_eq!(
                lua_file_strat.clone().resolve_relative(&envs).unwrap(),
                CommandStrategy::LuaFile {
                    path: Path::new("/path/to/clean.lua").as_os_str().to_os_string(),
                    args: vec![OsString::from("a")],
                }
            );
            assert!(lua_file_strat.resolve_relative(&empty_envs).is_err());

            let lua_strat = classify("#!lua return 1");
            assert_eq!(
                lua_strat.clone().resolve_relative(&empty_envs).unwrap(),
                CommandStrategy::Lua("return 1".to_string())
            );
        }
    }
}
