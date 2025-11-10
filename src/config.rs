use std::{fmt, ops::Deref, u8};

use crate::{impl_int_wrapper};

use crate::{Result, action::{Count}, binds::BindMap, tui::IoStream};
use ratatui::{
    style::{Color, Modifier}, widgets::{BorderType, Borders, Padding}
};
use regex::Regex;
use serde::{Deserialize, Deserializer, de::{self, Visitor, Error}};

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    // configure the ui
    #[serde(flatten)]
    pub render: RenderConfig,
    
    // binds
    pub binds: BindMap,
    
    // Ideally, this would deserialize flattened from PreviewConfig, but its way too much trouble
    pub previewer: PreviewerConfig,
    
    // instantiate the picker
    pub matcher: MatcherConfig,
    
    // similarly, maybe this would prefer to be in ui
    pub tui: TerminalConfig,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MatcherConfig {
    #[serde(flatten)]
    pub matcher: NucleoMatcherConfig,
    pub columns: ColumnsConfig,
    pub trim: bool,
    pub delimiter: Option<char>,
    #[serde(flatten)]
    pub exit: ExitConfig,
    pub format: FormatString
}

// for now, just stores all misc options for render_loop
#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExitConfig {
    pub select_1: bool,
    pub sync: bool
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RenderConfig {
    pub ui: UiConfig,
    pub input: InputConfig,
    pub results: ResultsConfig,
    pub preview: PreviewConfig,
}

impl RenderConfig {
    pub fn tick_rate(&self) -> u16 {
        self.ui.tick_rate.0
    }
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub border_fg: Color,
    pub background: Option<Color>,
    pub tick_rate: TickRate, // seperate from render, but best place ig
    
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub stream: IoStream,
    pub sleep: u16, // necessary to give ratatui a small delay before resizing after entering and exiting
    #[serde(flatten)]
    pub layout: Option<TerminalLayoutSettings> // None for fullscreen
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputConfig {
    pub input_fg: Color,
    pub count_fg: Color,
    pub cursor: CursorSetting,
    pub border: BorderSetting,
    pub title: String,
    #[serde(deserialize_with = "deserialize_char")]
    pub prompt: String,
    pub initial: String,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResultsConfig {
    #[serde(deserialize_with = "deserialize_char")]
    pub multi_prefix: String,
    pub default_prefix: String,
    #[serde(deserialize_with = "deserialize_option_bool")]
    pub reverse: Option<bool>,
    
    pub border: BorderSetting,
    pub result_fg: Color,
    pub current_fg: Color,
    pub current_bg: Color,
    pub match_fg: Color,
    pub count_fg: Color,
    #[serde(deserialize_with = "deserialize_modifier")]
    pub current_modifier: Modifier,
    
    #[serde(deserialize_with = "deserialize_modifier")]
    pub count_modifier: Modifier,
    
    pub title: String,
    pub scroll_wrap: bool,
    pub scroll_padding: u16,
    
    // experimental
    pub column_spacing: Count,
    pub current_prefix: String,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeaderConfig {
    pub border: BorderSetting,
    #[serde(deserialize_with = "deserialize_modifier")]
    pub modifier: Modifier,
    pub title: String,
    
    pub content: StringOrVec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>)
}
impl Default for StringOrVec {
    fn default() -> Self {
        StringOrVec::String(String::new())
    }
}


#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    pub border: BorderSetting,
    pub layout: Vec<PreviewSetting>,
    pub scroll_wrap: bool,
    pub wrap: bool,
    pub show: bool,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewerConfig {
    pub try_lossy: bool,
    
    // TODO
    pub wrap: bool,
    pub cache: bool,
    
}

// ----------- SETTING TYPES -------------------------
// Default config file -> write if not exists, then load

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct FormatString(String);

impl Deref for FormatString {
    type Target = str;
    
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BorderSetting {
    #[serde(deserialize_with = "fromstr_deserialize")]
    pub r#type: BorderType,
    pub color: Color,
    #[serde(deserialize_with = "deserialize_borders")]
    pub sides: Borders,
    #[serde(deserialize_with = "deserialize_padding")]
    pub padding: Padding,
    pub title: String,
}

impl BorderSetting {
    pub fn as_block(&self) -> ratatui::widgets::Block<'static> {
        let mut ret = ratatui::widgets::Block::default();
        
        if !self.title.is_empty() {
            let title = self.title.to_string();
            ret = ret.title(title)
        };
        
        if self.sides != Borders::NONE {
            ret = ret.borders(self.sides)
            .border_type(self.r#type)
            .border_style(ratatui::style::Style::default().fg(self.color))
        }
        
        ret
    }
    
    pub fn height(&self) -> u16 {
        let mut height = 0;
        height += 2 * !self.sides.is_empty() as u16;
        height += self.padding.top + self.padding.bottom;
        height += (!self.title.is_empty() as u16).saturating_sub(!self.sides.is_empty() as u16);
        
        height
    }
    
    pub fn width(&self) -> u16 {
        let mut width = 0;
        width += 2 * !self.sides.is_empty() as u16;
        width += self.padding.left + self.padding.right;
        width += (!self.title.is_empty() as u16).saturating_sub(!self.sides.is_empty() as u16);
        
        width
    }
}

// how to determine how many rows to allocate?
#[derive(Debug, Clone, Deserialize)]
pub struct TerminalLayoutSettings {
    pub percentage: Percentage,
    pub min: u16,
    pub max: u16, // 0 for terminal height cap
}

impl Default for TerminalLayoutSettings {
    fn default() -> Self {
        Self {
            percentage: Percentage(40),
            min: 10,
            max: 120
        }
    }
}


#[derive(Default, Debug, Clone, Deserialize)]
pub enum Side {
    Top,
    Bottom,
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewSetting {
    #[serde(flatten)]
    pub layout: PreviewLayoutSetting,
    #[serde(default)]
    pub command: String
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewLayoutSetting {
    pub side: Side,
    pub percentage: Percentage,
    pub min: i16,
    pub max: i16,
}

impl Default for PreviewLayoutSetting {
    fn default() -> Self {
        Self {
            side: Side::Right,
            percentage: Percentage(40),
            min: 30,
            max: 120
        }
    }
}


#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorSetting {
    None,
    #[default]
    Default,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct TickRate(pub u16);
impl Default for TickRate {
    fn default() -> Self {
        Self(60)
    }
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColumnsConfig {
    pub split: Split,
    pub names: Vec<ColumnSetting>,
    pub max_columns: MaxCols,
}

impl_int_wrapper!(MaxCols, u8, u8::MAX);

#[derive(Default, Debug, Clone)]
pub struct ColumnSetting {
    pub filter: bool,
    pub hidden: bool,
    pub name: String,
}

#[derive(Default, Debug, Clone)]
pub enum Split {
    Delimiter(Regex),
    Regexes(Vec<Regex>),
    #[default]
    None
}


// ----------- UTILS -------------------------

pub mod utils {
    use crate::Result;
    use crate::config::Config;
    use std::borrow::Cow;
    use std::path::{Path};
    
    pub fn get_config(dir: &Path) -> Result<Config> {
        let config_path = dir.join("config.toml");
        
        let config_content: Cow<'static, str> = if !config_path.exists() {
            Cow::Borrowed(include_str!("../assets/config.toml"))
        } else {
            Cow::Owned(std::fs::read_to_string(config_path)?)
        };
        
        let config: Config = toml::from_str(&config_content)?;
        
        Ok(config)
    }
    
    pub fn write_config(dir: &Path) -> Result<()> {
        let config_path = dir.join("config.toml");
        
        let default_config_content = include_str!("../assets/config.toml");
        let parent_dir = config_path.parent().unwrap();
        std::fs::create_dir_all(parent_dir)?;
        std::fs::write(&config_path, default_config_content)?;
        
        println!("Config written to: {}", config_path.display());
        Ok(())
    }
}

// --------- Deserialize Helpers ------------
fn fromstr_deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
T: std::str::FromStr,
T::Err: std::fmt::Display,
D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    T::from_str(&s).map_err(serde::de::Error::custom)
}



pub fn deserialize_borders<'de, D>(deserializer: D) -> Result<Borders, D::Error>
where
D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Input {
        Str(String),
        List(Vec<String>),
    }
    
    let input = Input::deserialize(deserializer)?;
    let mut borders = Borders::NONE;
    
    match input {
        Input::Str(s) => match s.as_str() {
            "none" => return Ok(Borders::NONE),
            "all" => return Ok(Borders::ALL),
            other => {
                return Err(de::Error::custom(format!(
                    "invalid border value '{}'",
                    other
                )));
            }
        },
        Input::List(list) => {
            for item in list {
                match item.as_str() {
                    "top" => borders |= Borders::TOP,
                    "bottom" => borders |= Borders::BOTTOM,
                    "left" => borders |= Borders::LEFT,
                    "right" => borders |= Borders::RIGHT,
                    "all" => borders |= Borders::ALL,
                    "none" => borders = Borders::NONE,
                    other => return Err(de::Error::custom(format!("invalid side '{}'", other))),
                }
            }
        }
    }
    
    Ok(borders)
}

pub fn deserialize_modifier<'de, D>(deserializer: D) -> Result<Modifier, D::Error>
where
D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Input {
        Str(String),
        List(Vec<String>),
    }
    
    let input = Input::deserialize(deserializer)?;
    let mut modifier = Modifier::empty();
    
    let add_modifier = |name: &str, m: &mut Modifier| -> Result<(), D::Error> {
        match name {
            "bold" => {
                *m |= Modifier::BOLD;
                Ok(())
            }
            "italic" => {
                *m |= Modifier::ITALIC;
                Ok(())
            }
            "underlined" => {
                *m |= Modifier::UNDERLINED;
                Ok(())
            }
            // "slow_blink" => {
            //     *m |= Modifier::SLOW_BLINK;
            //     Ok(())
            // }
            // "rapid_blink" => {
            //     *m |= Modifier::RAPID_BLINK;
            //     Ok(())
            // }
            // "reversed" => {
            //     *m |= Modifier::REVERSED;
            //     Ok(())
            // }
            // "dim" => {
            //     *m |= Modifier::DIM;
            //     Ok(())
            // }
            // "crossed_out" => {
            //     *m |= Modifier::CROSSED_OUT;
            //     Ok(())
            // }
            "none" => {
                *m = Modifier::empty();
                Ok(())
            } // reset all modifiers
            other => Err(de::Error::custom(format!("invalid modifier '{}'", other))),
        }
    };
    
    match input {
        Input::Str(s) => add_modifier(&s, &mut modifier)?,
        Input::List(list) => {
            for item in list {
                add_modifier(&item, &mut modifier)?;
            }
        }
    }
    
    Ok(modifier)
}

pub fn deserialize_char<'de, D>(deserializer: D) -> Result<String, D::Error>
where
D: Deserializer<'de>,
{
    struct CharVisitor;
    
    impl<'de> Visitor<'de> for CharVisitor {
        type Value = String;
        
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or single character")
        }
        
        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
        E: de::Error,
        {
            if v.chars().count() == 1 {
                let mut s = String::with_capacity(2);
                s.push(v.chars().next().unwrap());
                s.push(' ');
                Ok(s)
            } else {
                Ok(v.to_string())
            }
        }
        
        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
        E: de::Error,
        {
            self.visit_str(&v)
        }
    }
    
    deserializer.deserialize_string(CharVisitor)
}

// ----------- Nucleo config helper
#[derive(Debug, Clone)]
pub struct NucleoMatcherConfig(pub nucleo::Config);

impl Default for NucleoMatcherConfig {
    fn default() -> Self {
        Self(nucleo::Config::DEFAULT)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct MatcherConfigHelper {
    pub normalize: Option<bool>,
    pub ignore_case: Option<bool>,
    pub prefer_prefix: Option<bool>,
}

impl Default for MatcherConfigHelper {
    fn default() -> Self {
        Self {
            normalize: None,
            ignore_case: None,
            prefer_prefix: None,
        }
    }
}

impl<'de> Deserialize<'de> for NucleoMatcherConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
    D: serde::Deserializer<'de>,
    {
        let helper = MatcherConfigHelper::deserialize(deserializer)?;
        let mut config = nucleo::Config::DEFAULT;
        
        if let Some(norm) = helper.normalize {
            config.normalize = norm;
        }
        if let Some(ic) = helper.ignore_case {
            config.ignore_case = ic;
        }
        if let Some(pp) = helper.prefer_prefix {
            config.prefer_prefix = pp;
        }
        
        Ok(NucleoMatcherConfig(config))
    }
}

impl<'de> Deserialize<'de> for Split {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
    D: Deserializer<'de>,
    {
        struct SplitVisitor;
        
        impl<'de> Visitor<'de> for SplitVisitor {
            type Value = Split;
            
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("string for delimiter or array of strings for regexes")
            }
            
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
            E: de::Error,
            {
                // Try to compile single regex
                Regex::new(value)
                .map(Split::Delimiter)
                .map_err(|e| E::custom(format!("Invalid regex: {}", e)))
            }
            
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
            A: serde::de::SeqAccess<'de>,
            {
                let mut regexes = Vec::new();
                while let Some(s) = seq.next_element::<String>()? {
                    let r = Regex::new(&s).map_err(|e| de::Error::custom(format!("Invalid regex: {}", e)))?;
                    regexes.push(r);
                }
                Ok(Split::Regexes(regexes))
            }
        }
        
        deserializer.deserialize_any(SplitVisitor)
    }
}


impl<'de> Deserialize<'de> for ColumnSetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
    D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ColumnStruct {
            #[serde(default = "default_true")]
            filter: bool,
            #[serde(default)]
            hidden: bool,
            name: String,
        }
        
        fn default_true() -> bool { true }
        
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Str(String),
            Obj(ColumnStruct),
        }
        
        match Input::deserialize(deserializer)? {
            Input::Str(name) => Ok(ColumnSetting {
                filter: true,
                hidden: false,
                name,
            }),
            Input::Obj(obj) => Ok(ColumnSetting {
                filter: obj.filter,
                hidden: obj.hidden,
                name: obj.name,
            }),
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Percentage(u8);

impl Percentage {
    pub fn new(value: u8) -> Option<Self> {
        if value <= 100 {
            Some(Self(value))
        } else {
            None
        }
    }
    
    pub fn get(&self) -> u16 {
        self.0 as u16
    }
    
    pub fn get_max(&self, total: u16, max: u16) -> u16 {
        let pct_height = (total * self.get()).div_ceil(100);
        let max_height = if max == 0 { total } else { max };
        pct_height.min(max_height)
    }
}


impl Deref for Percentage {
    type Target = u8;
    
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Percentage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

// Implement Deserialize
impl<'de> Deserialize<'de> for Percentage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
    D: Deserializer<'de>,
    {
        let v = u8::deserialize(deserializer)?;
        if v <= 100 {
            Ok(Percentage(v))
        } else {
            Err(serde::de::Error::custom(format!("percentage out of range: {}", v)))
        }
    }
}
impl std::str::FromStr for Percentage {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim_end_matches('%'); // allow optional trailing '%'
        let value: u8 = s
        .parse()
        .map_err(|e: std::num::ParseIntError| format!("Invalid number: {}", e))?;
        Self::new(value).ok_or_else(|| format!("Percentage out of range: {}", value))
    }
}


pub fn deserialize_padding<'de, D>(deserializer: D) -> Result<Padding, D::Error>
where
D: Deserializer<'de>,
{
    struct PaddingVisitor;
    
    impl<'de> de::Visitor<'de> for PaddingVisitor {
        type Value = Padding;
        
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a number or an array of 1, 2, or 4 numbers")
        }
        
        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
        E: de::Error,
        {
            let v = value as u16;
            Ok(Padding { top: v, right: v, bottom: v, left: v })
        }
        
        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
        A: de::SeqAccess<'de>,
        {
            let first: u16 = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
            let second: Option<u16> = seq.next_element()?;
            let third: Option<u16> = seq.next_element()?;
            let fourth: Option<u16> = seq.next_element()?;
            
            match (second, third, fourth) {
                (None, None, None) => Ok(Padding { top: first, right: first, bottom: first, left: first }),
                (Some(v2), None, None) => Ok(Padding { top: v2, bottom: v2, left: first, right: first }),
                (Some(v2), Some(v3), Some(v4)) => Ok(Padding { top: first, right: v2, bottom: v3, left: v4 }),
                _ => Err(de::Error::invalid_length(2, &self)),
            }
        }
    }
    
    deserializer.deserialize_any(PaddingVisitor)
}

fn deserialize_option_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(match opt.as_deref() {
        Some("true") => Some(true),
        Some("false") => Some(false),
        Some("auto") => None,
        None => None,
        Some(other) => return Err(D::Error::custom(format!("invalid value: {}", other))),
    })
}