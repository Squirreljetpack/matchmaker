use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatcherBackendKind {
    #[default]
    Nucleo,
    Frizbee,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpecificMatcherConfig {
    Nucleo(nucleo::Config),
    Frizbee(nucleo::frizbee::Config),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatcherConfig {
    pub backend: MatcherBackendKind,
    pub specific: SpecificMatcherConfig,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            backend: MatcherBackendKind::Nucleo,
            specific: SpecificMatcherConfig::Nucleo(nucleo::Config::DEFAULT),
        }
    }
}

impl From<MatcherConfig> for nucleo::MatcherBackend {
    fn from(c: MatcherConfig) -> Self {
        match c.specific {
            SpecificMatcherConfig::Nucleo(cfg) => nucleo::MatcherBackend::Nucleo(cfg),
            SpecificMatcherConfig::Frizbee(cfg) => nucleo::MatcherBackend::Frizbee(cfg),
        }
    }
}

impl From<nucleo::Config> for MatcherConfig {
    fn from(config: nucleo::Config) -> Self {
        Self {
            backend: MatcherBackendKind::Nucleo,
            specific: SpecificMatcherConfig::Nucleo(config),
        }
    }
}

impl From<nucleo::frizbee::Config> for MatcherConfig {
    fn from(config: nucleo::frizbee::Config) -> Self {
        Self {
            backend: MatcherBackendKind::Frizbee,
            specific: SpecificMatcherConfig::Frizbee(config),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers & Deserialization
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct NucleoHelper {
    normalize: bool,
    ignore_case: bool,
    prefer_prefix: bool,
    match_paths: bool,
}

impl Default for NucleoHelper {
    fn default() -> Self {
        Self {
            normalize: nucleo::Config::DEFAULT.normalize,
            ignore_case: nucleo::Config::DEFAULT.ignore_case,
            prefer_prefix: nucleo::Config::DEFAULT.prefer_prefix,
            match_paths: false,
        }
    }
}

impl NucleoHelper {
    fn from_config(c: &nucleo::Config) -> Self {
        Self {
            normalize: c.normalize,
            ignore_case: c.ignore_case,
            prefer_prefix: c.prefer_prefix,
            match_paths: false,
        }
    }

    fn into_config(self) -> nucleo::Config {
        let mut cfg = nucleo::Config::DEFAULT;
        if self.match_paths {
            cfg.set_match_paths();
        }
        cfg.normalize = self.normalize;
        cfg.ignore_case = self.ignore_case;
        cfg.prefer_prefix = self.prefer_prefix;
        cfg
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(remote = "nucleo::frizbee::Scoring", default)]
struct FrizbeeScoringDef {
    match_score: u16,
    mismatch_penalty: u16,
    gap_open_penalty: u16,
    gap_extend_penalty: u16,
    prefix_bonus: u16,
    capitalization_bonus: u16,
    matching_case_bonus: u16,
    exact_match_bonus: u16,
    delimiter_bonus: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct FrizbeeHelper {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_typos: Option<u16>,
    #[serde(flatten, with = "FrizbeeScoringDef")]
    scoring: nucleo::frizbee::Scoring,
}

impl Default for FrizbeeHelper {
    fn default() -> Self {
        let default_cfg = nucleo::frizbee::Config::default();
        Self {
            max_typos: default_cfg.max_typos,
            scoring: default_cfg.scoring,
        }
    }
}

impl FrizbeeHelper {
    fn from_config(c: &nucleo::frizbee::Config) -> Self {
        Self {
            max_typos: c.max_typos,
            scoring: c.scoring.clone(),
        }
    }

    fn into_config(self) -> nucleo::frizbee::Config {
        nucleo::frizbee::Config {
            max_typos: self.max_typos,
            scoring: self.scoring,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "lowercase")]
enum TaggedMatcherHelper {
    Nucleo(NucleoHelper),
    Frizbee(FrizbeeHelper),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum MatcherHelperDe {
    Tagged(TaggedMatcherHelper),
    DefaultNucleo(NucleoHelper),
}

impl Serialize for MatcherConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.specific {
            SpecificMatcherConfig::Nucleo(cfg) => {
                TaggedMatcherHelper::Nucleo(NucleoHelper::from_config(cfg)).serialize(serializer)
            }
            SpecificMatcherConfig::Frizbee(cfg) => {
                TaggedMatcherHelper::Frizbee(FrizbeeHelper::from_config(cfg)).serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for MatcherConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (backend, specific) = match MatcherHelperDe::deserialize(deserializer)? {
            MatcherHelperDe::Tagged(TaggedMatcherHelper::Nucleo(h))
            | MatcherHelperDe::DefaultNucleo(h) => (
                MatcherBackendKind::Nucleo,
                SpecificMatcherConfig::Nucleo(h.into_config()),
            ),
            MatcherHelperDe::Tagged(TaggedMatcherHelper::Frizbee(h)) => (
                MatcherBackendKind::Frizbee,
                SpecificMatcherConfig::Frizbee(h.into_config()),
            ),
        };
        Ok(MatcherConfig { backend, specific })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matcher_config_deserialize_nucleo_default() {
        let toml_str = r#"
            ignore_case = false
            prefer_prefix = true
            match_paths = true
        "#;
        let config: MatcherConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.backend, MatcherBackendKind::Nucleo);
        match config.specific {
            SpecificMatcherConfig::Nucleo(c) => {
                assert_eq!(c.ignore_case, false);
                assert_eq!(c.prefer_prefix, true);
            }
            _ => panic!("expected nucleo config"),
        }
    }

    #[test]
    fn test_matcher_config_deserialize_frizbee() {
        let toml_str = r#"
            backend = "frizbee"
            max_typos = 2
            gap_open_penalty = 12
        "#;
        let config: MatcherConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.backend, MatcherBackendKind::Frizbee);
        match &config.specific {
            SpecificMatcherConfig::Frizbee(c) => {
                assert_eq!(c.max_typos, Some(2));
                assert_eq!(c.scoring.gap_open_penalty, 12);

                let haystacks = ["banana", "apple"];
                // "bannana" has a typo compared to haystack "banana"
                let matches = nucleo::frizbee::match_list("bannana", &haystacks, c);
                assert_eq!(matches.len(), 1);
                assert_eq!(haystacks[matches[0].index as usize], "banana");

                let mut strict_config = c.clone();
                strict_config.max_typos = Some(0);
                let strict_matches =
                    nucleo::frizbee::match_list("bannana", &haystacks, &strict_config);
                assert!(
                    strict_matches.is_empty(),
                    "expected 0 matches with max_typos = 0"
                );
            }
            _ => panic!("expected frizbee config"),
        }
        let backend: nucleo::MatcherBackend = config.into();
        match backend {
            nucleo::MatcherBackend::Frizbee(c) => {
                assert_eq!(c.max_typos, Some(2));
                assert_eq!(c.scoring.gap_open_penalty, 12);
            }
            _ => panic!("expected frizbee backend"),
        }
    }

    #[test]
    fn test_matcher_config_serialize_roundtrip() {
        let toml_str = r#"backend = "frizbee"
max_typos = 2
gap_open_penalty = 12
"#;
        let config: MatcherConfig = toml::from_str(toml_str).unwrap();
        let serialized = toml::to_string(&config).unwrap();
        let roundtrip: MatcherConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config, roundtrip);
    }
}
