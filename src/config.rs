use std::{fmt, ops::Deref};

use crate::utils::text::parse_escapes;
use crate::{impl_int_wrapper};

use crate::{Result, action::{Count}, binds::BindMap, tui::IoStream};
use ratatui::{
    style::{Color, Modifier}, widgets::{BorderType, Borders, Padding}
};
use regex::Regex;
use serde::Serialize;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, de::{self, Visitor, Error}};

// Note that serde deny is not supported with flatten

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    // configure the ui
    #[serde(flatten)]
    pub render: RenderConfig,

    // binds
    pub binds: BindMap,

    pub tui: TerminalConfig,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MainConfig {
    #[serde(flatten)]
    pub config: Config,

    pub previewer: PreviewerConfig,

    // this is in a bit of a awkward place because the matcher, worker, picker and reader all want pieces of it.
    pub matcher: MatcherConfig,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatcherConfig {
    #[serde(flatten)]
    pub matcher: NucleoMatcherConfig,
    #[serde(flatten)]
    pub mm: MMConfig,
    // only nit is that we would really prefer run config sit at top level since its irrelevant to MM::new_from_config
    #[serde(flatten)]
    pub run: StartConfig,

    #[serde(default)]
    pub help_colors: TomlColorConfig
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TomlColorConfig {
    pub section: Color,
    pub key: Color,
    pub string: Color,
    pub number: Color,
    pub section_bold: bool,
}

impl Default for TomlColorConfig {
    fn default() -> Self {
        Self {
            section: Color::Blue,
            key: Color::Yellow,
            string: Color::Green,
            number: Color::Cyan,
            section_bold: true,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MMConfig {
    pub columns: ColumnsConfig,
    #[serde(flatten)]
    pub exit: ExitConfig,
    pub format: FormatString, // todo: implement
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StartConfig {
    #[serde(default, deserialize_with = "parse_escaped_char_opt")]
    pub input_separator: Option<char>,
    #[serde(default, deserialize_with = "parse_escaped_opt")]
    pub output_separator: Option<String>,
    pub default_command: String,
    pub sync: bool
}


// stores all misc options for render_loop
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExitConfig {
    pub select_1: bool,
    pub allow_empty: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RenderConfig {
    pub ui: UiConfig,
    pub input: InputConfig,
    pub results: ResultsConfig,
    pub preview: PreviewConfig,
}

impl RenderConfig {
    pub fn tick_rate(&self) -> u8 {
        self.ui.tick_rate.0
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub border_fg: Color,
    pub background: Option<Color>,
    pub tick_rate: TickRate, // seperate from render, but best place ig
}
impl_int_wrapper!(TickRate, u8, 60);
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub stream: IoStream,
    pub sleep: u16, // necessary to give ratatui a small delay before resizing after entering and exiting
    #[serde(flatten)]
    pub layout: Option<TerminalLayoutSettings> // None for fullscreen
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResultsConfig {
    #[serde(deserialize_with = "deserialize_char")]
    pub multi_prefix: String,
    pub default_prefix: String,
    #[serde(deserialize_with = "deserialize_option_bool")]
    pub reverse: Option<bool>,
    pub wrap: bool,

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
    pub min_col_nowrap: MinColNoWrap,

    // experimental
    pub column_spacing: Count,
    pub current_prefix: String,
}

impl_int_wrapper!(MinColNoWrap, u8, 5);

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    pub border: BorderSetting,
    #[serde(deserialize_with = "deserialize_modifier")]
    pub modifier: Modifier,
    pub title: String,

    pub content: StringOrVec,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    pub border: BorderSetting,
    pub layout: Vec<PreviewSetting>,
    pub scroll_wrap: bool,
    pub wrap: bool,
    pub show: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewerConfig {
    pub try_lossy: bool,

    // TODO
    pub wrap: bool,
    pub cache: u8,

    pub help: String,

}

// ----------- SETTING TYPES -------------------------
// Default config file -> write if not exists, then load

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FormatString(String);

impl Deref for FormatString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BorderSetting {
    #[serde(with = "serde_from_str")]
    pub r#type: BorderType,
    pub color: Color,
    #[serde(serialize_with = "serialize_borders", deserialize_with = "deserialize_borders")]
    pub sides: Borders,
    #[serde(serialize_with = "serialize_padding", deserialize_with = "deserialize_padding")]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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


#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Top,
    Bottom,
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewSetting {
    #[serde(flatten)]
    pub layout: PreviewLayoutSetting,
    #[serde(default)]
    pub command: String
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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


#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorSetting {
    None,
    #[default]
    Default,
}


// todo: pass filter and hidden to mm
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColumnsConfig {
    pub split: Split,
    pub names: Vec<ColumnSetting>,
    pub max_columns: MaxCols,
}

impl_int_wrapper!(MaxCols, u8, u8::MAX);

#[derive(Default, Debug, Clone, PartialEq)]
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

impl PartialEq for Split {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Split::Delimiter(r1), Split::Delimiter(r2)) => r1.as_str() == r2.as_str(),
            (Split::Regexes(v1), Split::Regexes(v2)) => {
                if v1.len() != v2.len() { return false; }
                v1.iter().zip(v2.iter()).all(|(r1, r2)| r1.as_str() == r2.as_str())
            },
            (Split::None, Split::None) => true,
            _ => false,
        }
    }
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>)
}
impl Default for StringOrVec {
    fn default() -> Self {
        StringOrVec::String(String::new())
    }
}


// ----------- UTILS -------------------------

pub mod serde_from_str {
    use std::fmt::Display;
    use std::str::FromStr;
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Display,
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: FromStr,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        T::from_str(&s).map_err(de::Error::custom)
    }
}

pub mod utils {
    use crate::Result;
    use crate::config::{MainConfig};
    use std::borrow::Cow;
    use std::path::{Path};

    pub fn get_config(dir: &Path) -> Result<MainConfig> {
        let config_path = dir.join("config.toml");

        let config_content: Cow<'static, str> = if !config_path.exists() {
            Cow::Borrowed(include_str!("../assets/config.toml"))
        } else {
            Cow::Owned(std::fs::read_to_string(config_path)?)
        };

        let config: MainConfig = toml::from_str(&config_content)?;

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

    #[cfg(debug_assertions)]
    pub fn write_config_dev(dir: &Path) -> Result<()> {
        let config_path = dir.join("config.toml");

        let default_config_content = include_str!("../assets/dev.toml");
        let parent_dir = config_path.parent().unwrap();
        std::fs::create_dir_all(parent_dir)?;
        std::fs::write(&config_path, default_config_content)?;

        Ok(())
    }
}



// --------- Deserialize Helpers ------------
pub fn serialize_borders<S>(borders: &Borders, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(None)?;
    if borders.contains(Borders::TOP) {
        seq.serialize_element("top")?;
    }
    if borders.contains(Borders::BOTTOM) {
        seq.serialize_element("bottom")?;
    }
    if borders.contains(Borders::LEFT) {
        seq.serialize_element("left")?;
    }
    if borders.contains(Borders::RIGHT) {
        seq.serialize_element("right")?;
    }
    seq.end()
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
        match name.to_lowercase().as_str() {
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
#[derive(Debug, Clone, PartialEq)]
pub struct NucleoMatcherConfig(pub nucleo::Config);

impl Default for NucleoMatcherConfig {
    fn default() -> Self {
        Self(nucleo::Config::DEFAULT)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
struct MatcherConfigHelper {
    pub normalize: Option<bool>,
    pub ignore_case: Option<bool>,
    pub prefer_prefix: Option<bool>,
}


impl serde::Serialize for NucleoMatcherConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let helper = MatcherConfigHelper {
            normalize: Some(self.0.normalize),
            ignore_case: Some(self.0.ignore_case),
            prefer_prefix: Some(self.0.prefer_prefix),
        };
        helper.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NucleoMatcherConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,    {
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

impl serde::Serialize for Split {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Split::Delimiter(r) => serializer.serialize_str(r.as_str()),
            Split::Regexes(rs) => {
                let mut seq = serializer.serialize_seq(Some(rs.len()))?;
                for r in rs {
                    seq.serialize_element(r.as_str())?;
                }
                seq.end()
            }
            Split::None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for Split {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,    {
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


impl serde::Serialize for ColumnSetting {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ColumnSetting", 3)?;
        state.serialize_field("filter", &self.filter)?;
        state.serialize_field("hidden", &self.hidden)?;
        state.serialize_field("name", &self.name)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ColumnSetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,    {
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


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
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


pub fn serialize_padding<S>(padding: &Padding, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    if padding.top == padding.bottom && padding.left == padding.right && padding.top == padding.left {
        serializer.serialize_u16(padding.top)
    } else if padding.top == padding.bottom && padding.left == padding.right {
        let mut seq = serializer.serialize_seq(Some(2))?;
        seq.serialize_element(&padding.left)?;
        seq.serialize_element(&padding.top)?;
        seq.end()
    } else {
        let mut seq = serializer.serialize_seq(Some(4))?;
        seq.serialize_element(&padding.top)?;
        seq.serialize_element(&padding.right)?;
        seq.serialize_element(&padding.bottom)?;
        seq.serialize_element(&padding.left)?;
        seq.end()
    }
}

pub fn deserialize_padding<'de, D>(deserializer: D) -> Result<Padding, D::Error>
where
    D: Deserializer<'de>,
{
    struct PaddingVisitor;

    impl<'de> Visitor<'de> for PaddingVisitor {
        type Value = Padding;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a number or an array of 1, 2, or 4 numbers")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let v = u16::try_from(value)
                .map_err(|_| E::custom(format!("padding value {} is out of range for u16", value)))?;

            Ok(Padding {
                top: v,
                right: v,
                bottom: v,
                left: v,
            })
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let v = u16::try_from(value)
                .map_err(|_| E::custom(format!("padding value {} is out of range for u16", value)))?;

            Ok(Padding {
                top: v,
                right: v,
                bottom: v,
                left: v,
            })
        }

        // 3. Handle Sequences (Arrays)
        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let first: u16 = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(0, &self))?;

            let second: Option<u16> = seq.next_element()?;
            let third: Option<u16> = seq.next_element()?;
            let fourth: Option<u16> = seq.next_element()?;

            match (second, third, fourth) {
                (None, None, None) => Ok(Padding {
                    top: first,
                    right: first,
                    bottom: first,
                    left: first,
                }),
                (Some(v2), None, None) => Ok(Padding {
                    top: first,
                    bottom: first,
                    left: v2,
                    right: v2,
                }),
                (Some(v2), Some(v3), Some(v4)) => Ok(Padding {
                    top: first,
                    right: v2,
                    bottom: v3,
                    left: v4,
                }),
                _ => Err(de::Error::invalid_length(3, &self)),
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

fn parse_escaped_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.map(|s| parse_escapes(&s)))
}

fn parse_escaped_char_opt<'de, D>(deserializer: D) -> Result<Option<char>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let parsed = parse_escapes(&s);
            let mut chars = parsed.chars();
            let first = chars.next().ok_or_else(|| {
                serde::de::Error::custom("escaped string is empty")
            })?;
            if chars.next().is_some() {
                return Err(serde::de::Error::custom(
                    "escaped string must be exactly one character",
                ));
            }
            Ok(Some(first))
        }
        None => Ok(None),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use toml;

    #[test]
    fn config_round_trip() {
        let default_toml = include_str!("../assets/dev.toml");
        let config: MainConfig = toml::from_str(default_toml)
            .expect("failed to parse default TOML");
        let serialized = toml::to_string_pretty(&config)
            .expect("failed to serialize to TOML");
        let deserialized: MainConfig = toml::from_str(&serialized)
            .expect("failed to parse serialized TOML");

        // Assert the round-trip produces the same data
        assert_eq!(config, deserialized);
    }
}