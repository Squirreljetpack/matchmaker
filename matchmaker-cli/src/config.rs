use serde::{Deserialize, Serialize};

use matchmaker::config::*;
use matchmaker_partial_macros::partial;

use matchmaker::action::Actions;
use matchmaker::binds::Trigger;
use std::{collections::HashMap, ffi::OsString};

use crate::action::MMAction;
use crate::sort::SortMode;

#[derive(Clone, PartialEq, Serialize)]
#[partial(recurse, path)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    // configure the ui
    #[partial(attr)]
    #[serde(default)]
    #[serde(flatten)]
    pub render: RenderConfig,

    #[serde(default)]
    #[serde(alias = "env")]
    #[partial(no_recurse, unwrap)]
    pub envs: HashMap<String, EnvValue>,

    // configure binds ( keypress/mouseevent/event => Actions )
    #[partial(attr)]
    #[serde(default)]
    #[partial(alias = "b")]
    #[partial(no_recurse, unwrap)]
    pub binds: HashMap<Trigger, Actions<MMAction>>,

    // configure the tui
    #[partial(attr)]
    #[serde(default)]
    pub tui: TerminalConfig,

    // configure the preview command runner
    #[serde(default)]
    pub previewer: PreviewerConfig,

    // configure the matcher (columns + matching settings)
    #[partial(attr, alias = "m")]
    #[serde(default)]
    pub matcher: MatcherConfig,

    // configure startup settings (options for how input/output is processed)
    #[partial(attr, alias = "s")]
    #[serde(default)]
    pub start: StartConfig,

    #[partial(attr, alias = "c")]
    #[serde(default)]
    /// How columns are parsed from input lines
    pub columns: ColumnsConfig,

    // configure exit conditions
    #[partial(attr, alias = "e")]
    #[serde(default)]
    pub exit: ExitConfig,

    // configure the pager used by ShowPreview (pages the current preview command
    // fullscreen via `minus`); skipped by the partial macro
    #[partial(skip)]
    #[cfg(feature = "pager")]
    #[serde(default)]
    pub pager: PagerConfig,

    /// imports: only supported on overrides and with one nesting level
    #[serde(default)]
    #[partial(no_recurse)]
    pub source: Option<std::path::PathBuf>,
}

// -----------------------

/// Settings unrelated to event loop/picker_ui.
///
/// Does not deny unknown fields.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[partial(path, derive(Debug, Deserialize))]
pub struct MatcherConfig {
    #[serde(flatten)]
    #[partial(skip)]
    pub matcher: NucleoMatcherConfig,
    /// Configures how input is fed to the worker(s).
    #[serde(flatten)]
    #[partial(recurse)]
    pub preprocess: PreprocessConfig,
    /// Startup sort settings, applied to the worker on start.
    #[partial(recurse)]
    #[serde(default)]
    pub sort: SortSetting,
    /// TODO: Enable raw mode where non-matching items are also displayed in a dimmed color.
    #[partial(alias = "r")]
    #[serde(default)]
    #[allow(dead_code)]
    pub raw: bool,
    /// TODO: Track the current selection when the result list is updated.
    #[serde(default)]
    #[allow(dead_code)]
    pub track: bool,
}

/// Startup sort settings, applied to the worker on start and mutated at
/// runtime by the `Sort`/`SortNumeric`/`SortReverse` actions.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Deserialize))]
pub struct SortSetting {
    /// Reverse the sort direction.
    pub reverse: bool,
    /// Sort mode; `None` (the default) keeps the input order.
    pub mode: SortMode,
    /// Name of the column to sort by; empty uses the primary column.
    pub column: String,
    /// How "stable" the results are. Higher values prioritize the initial ordering.
    pub threshold: SortThreshold,
}

/// (client-app responsibility). Configures how input is fed to to the worker(s).
/// Unfortunately, we cannot use deny_unknown_fields if we want to flatten PreprocessConfig
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct StartConfig {
    #[serde(deserialize_with = "escaped_opt_char")]
    #[partial(alias = "is")]
    pub input_separator: Option<char>,

    /// Print accepted items as.
    #[serde(deserialize_with = "escaped_opt_string")]
    #[partial(alias = "os")]
    pub output_separator: Option<String>,
    /// Format string to print accepted items as.
    #[partial(alias = "ot")]
    #[serde(alias = "output")]
    pub output_template: Option<String>,
    /// Execution template for accepted items. Exclusive with output_template and output_separator.
    pub on_accept: String,

    /// Default command to execute when stdin is not being read.
    #[partial(alias = "cmd", alias = "x")]
    pub command: CommandSetting,
    /// Additional command which can be cycled through using Action::ReloadNext
    #[partial(alias = "ax")]
    pub additional_commands: Vec<String>,

    /// Execution directory
    #[partial(alias = "d")]
    pub directory: EnvValue,

    pub sync: bool,

    /// Override the default mode
    pub mode: Option<String>,

    /// Shell to execute scripts with, e.g. `["bash", "-c"]`. Empty (the
    /// default) uses `$SHELL` (or `/bin/sh`).
    #[serde(deserialize_with = "os_strings::deserialize")]
    pub shell: Vec<OsString>,

    /// Don't kill the last populating command when reloading
    pub save_orphans: bool,
    /// If false, aborts program when encountering an invalid utf-8 input line
    pub skip_invalid_lines: bool,
}

// -----------------------

#[cfg(not(windows))]
pub const DEFAULT_CONFIG: &str = include_str!("../assets/config.toml");
#[cfg(windows)]
pub const DEFAULT_CONFIG: &str = include_str!("../assets/win.config.toml");

impl Default for Config {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).unwrap()
    }
}

/// Knobs for the `ShowPreview` pager (`[pager]` section).
///
/// Only present when the `pager` feature is enabled; without it, `ShowPreview`
/// falls back to the external pager chain `MM_PAGER -> $PAGER -> less -> more`.
#[cfg_attr(not(feature = "pager"), allow(dead_code))]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PagerConfig {
    /// Show line numbers in the pager.
    pub line_numbers: bool,
    /// Start the pager in follow mode (auto-scroll as new output arrives).
    pub follow: bool,
    /// Footer prompt text shown by the pager.
    pub prompt: Option<String>,
    /// Always enable horizontal scrolling (Ctrl+h still toggles it).
    pub horizontal_scroll: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let default_toml = include_str!("../assets/config.toml");
        let config: Config = toml::from_str(default_toml).expect("failed to parse default TOML");
        let serialized = toml::to_string_pretty(&config).expect("failed to serialize to TOML");
        let deserialized: Config = toml::from_str(&serialized)
            .unwrap_or_else(|e| panic!("failed to parse serialized TOML:\n{}\n{e}", serialized));

        // Assert the round-trip produces the same data
        assert_eq!(config, deserialized);
    }

    #[test]
    fn dev_config_round_trip() {
        let default_toml = include_str!("../assets/dev.toml");
        let config: Config = toml::from_str(default_toml).expect("failed to parse default TOML");
        let serialized = toml::to_string_pretty(&config).expect("failed to serialize to TOML");
        let deserialized: Config = toml::from_str(&serialized)
            .unwrap_or_else(|e| panic!("failed to parse serialized TOML:\n{}\n{e}", serialized));

        // Assert the round-trip produces the same data
        assert_eq!(config, deserialized);
    }
}
