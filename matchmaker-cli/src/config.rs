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

    // binds
    #[serde(default = "BindMap::default_binds")]
    #[partial(attr)]
    #[partial(alias = "b")]
    #[partial(recurse = "", unwrap)]
    // #[partial(skip)]
    pub binds: BTreeMap<Trigger, Actions<MMAction>>,

    #[serde(default)]
    #[partial(attr)]
    pub tui: TerminalConfig,

    #[serde(default)]
    #[partial(skip)]
    pub previewer: PreviewerConfig,

    // this is in a bit of a awkward place because the matcher, worker, picker and reader all want pieces of it.
    #[serde(default)]
    #[partial(attr, alias = "m")]
    pub matcher: MatcherConfig,

    #[serde(default)]
    #[partial(attr, alias = "s")]
    pub start: StartConfig,

    #[serde(default)]
    #[partial(attr, alias = "e")]
    pub exit: ExitConfig,
}

// ----------------------- CONFIG
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

// --------------------------------------------------------------------------------

// this was the original config but the generic is useless
// use matchmaker::action::{ActionExt, NullActionExt};
// trait ActionExt_: ActionExt + std::fmt::Display + std::str::FromStr {}
// impl<T: ActionExt + std::fmt::Display + std::str::FromStr> ActionExt_ for T {}
// #[allow(private_bounds)] // serde bound workaround
// /// Full config.
// /// Clients probably want to make their own type with RenderConfig + custom settings to instantiate their matchmaker.
// /// Used by the instantiation method [`crate::Matchmaker::new_from_config`]
// // #[partial(recurse, path, derive(Debug, Deserialize))]
// #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// #[serde(deny_unknown_fields, bound(serialize = "", deserialize = "",))]
// pub struct Config<A: ActionExt_ = NullActionExt> {
//     // Default bound on A, see https://github.com/serde-rs/serde/issues/1541
//     // configure the ui
//     #[serde(default, flatten)]
//     pub render: RenderConfig,

//     // binds
//     #[serde(default = "BindMap::default_binds", alias = "b")]
//     pub binds: BindMap<A>,

//     #[serde(default)]
//     pub tui: TerminalConfig,

//     #[serde(default)]
//     #[partial(skip)]
//     pub previewer: PreviewerConfig,

//     // this is in a bit of a awkward place because the matcher, worker, picker and reader all want pieces of it.
//     #[serde(default)]
//     pub matcher: MatcherConfig,
// }
