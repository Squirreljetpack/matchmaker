use std::{
    fmt::{self, Display},
    str::FromStr,
};

/// A prefix-based filter for matching [`crate::MODE`]
///
/// Positive prefixes (e.g., `"0"`) require the mode to contain a tag starting with that prefix.
/// Negative prefixes (e.g., `"!0"`) require the mode to NOT contain any tag starting with that prefix.
///
/// An empty filter (no positive or negative prefixes) matches every input mode string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct PrefixFilter {
    pub positive_prefixes: Vec<String>,
    pub negative_prefixes: Vec<String>,
}

impl PrefixFilter {
    /// Construct a `PrefixFilter` from a list of pattern strings.
    /// Patterns starting with `!` are treated as negative prefixes.
    pub fn from(patterns: Vec<&str>) -> Result<Self, String> {
        let mut positive_prefixes = Vec::new();
        let mut negative_prefixes = Vec::new();

        for pat in patterns {
            if let Some(pat) = pat.strip_prefix('!') {
                negative_prefixes.push(pat.to_string());
            } else {
                positive_prefixes.push(pat.to_string());
            }
        }

        Ok(Self {
            positive_prefixes,
            negative_prefixes,
        })
    }

    /// Check if this filter matches the given mode stack.
    pub fn matches(&self, mode: &[Box<str>]) -> bool {
        for prefix in &self.positive_prefixes {
            if !mode.iter().any(|tag| tag.as_ref().starts_with(prefix)) {
                log::trace!("{self:?}, {mode:?}");
                return false;
            }
        }

        for tag in mode {
            for prefix in &self.negative_prefixes {
                if tag.as_ref().starts_with(prefix) {
                    return false;
                }
            }
        }

        true
    }

    /// Returns true if this filter has no positive or negative prefixes.
    pub fn is_empty(&self) -> bool {
        self.positive_prefixes.is_empty() && self.negative_prefixes.is_empty()
    }
}

impl Display for PrefixFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut patterns = Vec::new();
        for p in &self.positive_prefixes {
            patterns.push(p.clone());
        }
        for p in &self.negative_prefixes {
            patterns.push(format!("!{p}"));
        }
        write!(f, "{}", patterns.join(","))
    }
}

impl FromStr for PrefixFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let patterns: Vec<&str> = s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        Self::from(patterns)
    }
}
