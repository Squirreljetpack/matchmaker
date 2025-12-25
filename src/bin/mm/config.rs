use serde::{Deserialize, Serialize};

use matchmaker::action::{ActionExt, NullActionExt};
use matchmaker::config::{MatcherConfig, PreviewerConfig, RenderConfig, TerminalConfig};
use matchmaker::{Result, binds::BindMap};

/// Full config.
/// Clients probably want to make their own type with RenderConfig + custom settings to instantiate their matchmaker.
/// Used by the instantiation method [`crate::Matchmaker::new_from_config`]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, bound(serialize = "", deserialize = "",))]
pub struct Config<A: ActionExt + Default = NullActionExt> {
    // Default bound on A, see https://github.com/serde-rs/serde/issues/1541
    // configure the ui
    #[serde(flatten)]
    pub render: RenderConfig,

    // binds
    pub binds: BindMap<A>,

    pub tui: TerminalConfig,

    pub previewer: PreviewerConfig,

    // this is in a bit of a awkward place because the matcher, worker, picker and reader all want pieces of it.
    pub matcher: MatcherConfig,
}

use std::borrow::Cow;
use std::path::Path;

pub fn get_config(path: &Path) -> Result<Config> {
    let config_content: Cow<'static, str> = if !path.exists() {
        Cow::Borrowed(include_str!("../../../assets/config.toml"))
    } else {
        Cow::Owned(std::fs::read_to_string(path)?)
    };

    let config: Config = toml::from_str(&config_content)?;

    Ok(config)
}

pub fn write_config(path: &Path) -> Result<()> {

    let default_config_content = include_str!("../../../assets/config.toml");
    let parent_dir = path.parent().unwrap();
    std::fs::create_dir_all(parent_dir)?;
    std::fs::write(path, default_config_content)?;

    println!("Config written to: {}", path.display());
    Ok(())
}

#[cfg(debug_assertions)]
pub fn write_config_dev(path: &Path) -> Result<()> {
    let default_config_content = include_str!("../../../assets/dev.toml");
    let parent_dir = path.parent().unwrap();
    std::fs::create_dir_all(parent_dir)?;
    std::fs::write(path, default_config_content)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let default_toml = include_str!("../../../assets/dev.toml");
        let config: Config = toml::from_str(default_toml).expect("failed to parse default TOML");
        let serialized = toml::to_string_pretty(&config).expect("failed to serialize to TOML");
        let deserialized: Config = toml::from_str(&serialized)
            .unwrap_or_else(|e| panic!("failed to parse serialized TOML:\n{}\n{e}", serialized));

        // Assert the round-trip produces the same data
        assert_eq!(config, deserialized);
    }
}
