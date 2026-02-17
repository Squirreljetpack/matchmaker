use std::fmt;

use cli_boilerplate_automation::define_restricted_wrapper;
use serde::{Deserialize, Deserializer};

define_restricted_wrapper!(
    #[derive(Clone)]
    Percentage: u16 = 100
);
impl Percentage {
    pub fn new(value: u16) -> Self {
        if value <= 100 { Self(value) } else { Self(100) }
    }

    pub fn compute_clamped(&self, total: u16, min: u16, max: u16) -> u16 {
        let pct_height = (total * self.inner()).div_ceil(100);
        pct_height.clamp(min, if max == 0 { total } else { max })
    }

    pub fn complement(&self) -> Self {
        Self(100 - self.0)
    }
}

impl fmt::Display for Percentage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

impl TryFrom<u16> for Percentage {
    type Error = String;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value > 100 {
            Err(format!("Percentage out of range: {}", value))
        } else {
            Ok(Self::new(value))
        }
    }
}
impl<'de> Deserialize<'de> for Percentage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = u16::deserialize(deserializer)?;
        v.try_into().map_err(serde::de::Error::custom)
    }
}
impl std::str::FromStr for Percentage {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim_end_matches('%'); // allow optional trailing '%'
        let v: u16 = s
            .parse()
            .map_err(|e: std::num::ParseIntError| format!("Invalid number: {}", e))?;
        v.try_into()
    }
}
