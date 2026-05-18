use serde::{Deserialize, Serialize};

use matchmaker::config::*;
use matchmaker_partial_macros::partial;

use matchmaker::action::Actions;
use matchmaker::binds::Trigger;
use std::collections::HashMap;

use crate::action::MMAction;

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

    // configure binds ( keypress/mouseevent/event => Actions )
    #[partial(attr)]
    #[serde(default)]
    #[partial(alias = "b")]
    #[partial(recurse = "", unwrap)]
    pub binds: HashMap<Trigger, Actions<MMAction>>,

    // configure the tui
    #[partial(attr)]
    #[serde(default)]
    pub tui: TerminalConfig,

    // configure the preview command runner
    #[partial(skip)]
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

    // configure exit conditions
    #[partial(attr, alias = "e")]
    #[serde(default)]
    pub exit: ExitConfig,

    #[partial(attr, alias = "c")]
    #[serde(default)]
    /// How columns are parsed from input lines
    pub columns: ColumnsConfig,
}

// -----------------------

#[cfg(not(windows))]
pub const DEFAULT_CONFIG: &str = include_str!("../assets/config.toml");
#[cfg(windows)]
pub const DEFAULT_CONFIG: &str = include_str!("../assets/config.win.toml");

impl Default for Config {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let default_toml = include_str!("../assets/dev.toml");
        let config: Config = toml::from_str(default_toml).expect("failed to parse default TOML");
        let serialized = toml::to_string_pretty(&config).expect("failed to serialize to TOML");
        let deserialized: Config = toml::from_str(&serialized)
            .unwrap_or_else(|e| panic!("failed to parse serialized TOML:\n{}\n{e}", serialized));

        // Assert the round-trip produces the same data
        assert_eq!(config, deserialized);
    }
}
