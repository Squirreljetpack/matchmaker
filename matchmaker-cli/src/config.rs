use serde::{Deserialize, Serialize};

use matchmaker_partial_macros::partial;

use matchmaker::binds::{BindMap, BindMapExt};
use matchmaker::config::*;

use matchmaker::action::Actions;
use matchmaker::binds::Trigger;
use std::collections::BTreeMap;

use crate::action::MMAction;

#[derive(Clone, PartialEq, Serialize)]
#[partial(recurse, path)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    // configure the ui
    #[serde(default, flatten)]
    #[partial(attr)]
    pub render: RenderConfig,

    // configure binds ( keypress/mouseevent/event => Actions )
    #[serde(default = "BindMap::default_binds")]
    #[partial(attr)]
    #[partial(alias = "b")]
    #[partial(recurse = "", unwrap)]
    pub binds: BTreeMap<Trigger, Actions<MMAction>>,

    // configure the tui
    #[serde(default)]
    #[partial(attr)]
    pub tui: TerminalConfig,

    // configure the preview command runner
    #[serde(default)]
    #[partial(skip)]
    pub previewer: PreviewerConfig,

    // configure the matcher (columns + matching settings)
    #[serde(default)]
    #[partial(attr, alias = "m")]
    pub matcher: MatcherConfig,

    // configure startup settings (options for how input/output is processed)
    #[serde(default)]
    #[partial(attr, alias = "s")]
    pub start: StartConfig,

    // configure exit conditions
    #[serde(default)]
    #[partial(attr, alias = "e")]
    pub exit: ExitConfig,
}

// -----------------------

impl Default for Config {
    fn default() -> Self {
        toml::from_str(include_str!("../assets/config.toml")).unwrap()
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
