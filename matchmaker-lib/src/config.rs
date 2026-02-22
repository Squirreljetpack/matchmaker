//! Config Types.
//! See `src/bin/mm/config.rs` for an example

use matchmaker_partial_macros::partial;

use std::{fmt, ops::Deref};

pub use crate::utils::{HorizontalSeparator, Percentage, serde::StringOrVec};

use crate::{
    MAX_SPLITS,
    tui::IoStream,
    utils::serde::{escaped_opt_char, escaped_opt_string, serde_duration_ms},
};

use cli_boilerplate_automation::define_transparent_wrapper;
use cli_boilerplate_automation::serde::{
    // one_or_many,
    through_string,
    transform::camelcase_normalized,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{BorderType, Borders, Padding},
};

use regex::Regex;

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, IntoDeserializer, Visitor},
    ser::SerializeSeq,
};

/// Settings unrelated to event loop/picker_ui.
///
/// Does not deny unknown fields.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[partial(recurse, path, derive(Debug, Deserialize))]
pub struct MatcherConfig {
    #[serde(flatten)]
    #[partial(skip)]
    pub matcher: NucleoMatcherConfig,
    #[serde(flatten)]
    pub worker: WorkerConfig,
    // only nit is that we would really prefer this config sit at top level since its irrelevant to [`crate::Matchmaker::new_from_config`] but it makes more sense under matcher in the config file
    #[serde(flatten)]
    pub start: StartConfig,
    #[serde(flatten)]
    pub exit: ExitConfig,
}

/// "Input/output specific". Configures the matchmaker worker.
///
/// Does not deny unknown fields.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Deserialize))]
pub struct WorkerConfig {
    #[partial(recurse)]
    #[partial(alias = "c")]
    /// How columns are parsed from input lines
    pub columns: ColumnsConfig,
    /// Trim the input
    #[partial(alias = "t")]
    pub trim: bool,
    /// How "stable" the results are. Higher values prioritize the initial ordering.
    pub sort_threshold: u32,
    /// Whether to parse ansi sequences from input
    #[partial(alias = "a")]
    pub ansi: bool,

    /// TODO: Enable raw mode where non-matching items are also displayed in a dimmed color.
    #[partial(alias = "r")]
    pub raw: bool,
    /// TODO: Track the current selection when the result list is updated.
    pub track: bool,
    /// TODO: Reverse the order of the input
    pub reverse: bool,
}

/// Configures how input is fed to to the worker(s).
///
/// Does not deny unknown fields.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Deserialize))]
pub struct StartConfig {
    #[serde(deserialize_with = "escaped_opt_char")]
    #[partial(alias = "is")]
    pub input_separator: Option<char>,
    #[serde(deserialize_with = "escaped_opt_string")]
    #[partial(alias = "os")]
    pub output_separator: Option<String>,

    /// Format string to print accepted items as.
    pub output_template: Option<String>,

    /// Default command to execute when stdin is not being read.
    #[partial(alias = "cmd", alias = "x")]
    pub command: String,
    #[partial(alias = "s")]
    pub sync: bool,
}

/// Exit conditions of the render loop.
///
/// Does not deny unknown fields.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Deserialize))]
pub struct ExitConfig {
    /// Exit automatically if there is only one match.
    pub select_1: bool,
    /// Allow returning without any items selected.
    pub allow_empty: bool,
    /// Abort if no items.
    pub abort_empty: bool,
}

/// The ui config.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(recurse, path, derive(Debug, Deserialize))]
pub struct RenderConfig {
    /// The default overlay style
    pub ui: UiConfig,
    /// The input bar style
    pub input: InputConfig,
    /// The results table style
    #[partial(alias = "r")]
    pub results: ResultsConfig,
    /// The preview panel style
    #[partial(alias = "p")]
    pub preview: PreviewConfig,
    pub footer: DisplayConfig,
    #[partial(alias = "h")]
    pub header: DisplayConfig,
}

impl RenderConfig {
    pub fn tick_rate(&self) -> u8 {
        self.ui.tick_rate
    }
}

/// Terminal settings.
#[partial(path, derive(Debug, Deserialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub stream: IoStream, // consumed
    pub restore_fullscreen: bool,
    pub redraw_on_resize: bool,
    // https://docs.rs/crossterm/latest/crossterm/event/struct.PushKeyboardEnhancementFlags.html
    pub extended_keys: bool,
    #[serde(with = "serde_duration_ms")]
    pub sleep_ms: std::time::Duration, // necessary to give ratatui a small delay before resizing after entering and exiting
    // todo: lowpri: will need a value which can deserialize to none when implementing cli parsing
    #[serde(flatten)]
    #[partial(recurse)]
    pub layout: Option<TerminalLayoutSettings>, // None for fullscreen
    pub clear_on_exit: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            stream: IoStream::default(),
            restore_fullscreen: true,
            redraw_on_resize: bool::default(),
            sleep_ms: std::time::Duration::default(),
            layout: Option::default(),
            extended_keys: true,
            clear_on_exit: true,
        }
    }
}
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalSettings {}

/// The container ui.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Deserialize))]
pub struct UiConfig {
    #[partial(recurse)]
    pub border: BorderSetting,
    pub tick_rate: u8, // separate from render, but best place ig
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            border: Default::default(),
            tick_rate: 60,
        }
    }
}

/// The input bar ui.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Deserialize))]
pub struct InputConfig {
    #[partial(recurse)]
    pub border: BorderSetting,

    // text styles
    #[serde(deserialize_with = "camelcase_normalized")]
    pub fg: Color,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub modifier: Modifier,

    #[serde(deserialize_with = "camelcase_normalized")]
    pub prompt_fg: Color,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub prompt_modifier: Modifier,

    #[serde(deserialize_with = "deserialize_string_or_char_as_double_width")]
    pub prompt: String,
    pub cursor: CursorSetting,
    pub initial: String,

    /// Maintain padding when moving the cursor in the bar.
    pub scroll_padding: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            border: Default::default(),
            fg: Default::default(),
            modifier: Default::default(),
            prompt_fg: Default::default(),
            prompt_modifier: Default::default(),
            prompt: "> ".to_string(),
            cursor: Default::default(),
            initial: Default::default(),

            scroll_padding: true,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Deserialize))]
pub struct OverlayConfig {
    #[partial(recurse)]
    pub border: BorderSetting,
    pub outer_dim: bool,
    pub layout: OverlayLayoutSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[partial(path, derive(Debug, Deserialize))]
pub struct OverlayLayoutSettings {
    /// w, h
    #[partial(alias = "p")]
    pub percentage: [Percentage; 2],
    /// w, h
    pub min: [u16; 2],
    /// w, h
    pub max: [u16; 2],
}

impl Default for OverlayLayoutSettings {
    fn default() -> Self {
        Self {
            percentage: [Percentage::new(60), Percentage::new(30)],
            min: [10, 5],
            max: [200, 30],
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub enum RowConnectionStyle {
    #[default]
    Disjoint,
    Capped,
    Full,
}

// pub struct OverlaySize

#[partial(path, derive(Debug, Deserialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResultsConfig {
    #[partial(recurse)]
    pub border: BorderSetting,

    // prefixes
    #[serde(deserialize_with = "deserialize_string_or_char_as_double_width")]
    pub multi_prefix: String,
    pub default_prefix: String,

    // text styles
    #[serde(deserialize_with = "camelcase_normalized")]
    pub fg: Color,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub modifier: Modifier,

    #[serde(deserialize_with = "camelcase_normalized")]
    pub match_fg: Color,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub match_modifier: Modifier,

    /// foreground of the current item.
    #[serde(deserialize_with = "camelcase_normalized")]
    pub current_fg: Color,
    /// background of the current item.
    #[serde(deserialize_with = "camelcase_normalized")]
    pub current_bg: Color,
    /// modifier of the current item.
    // #[serde(deserialize_with = "transform_uppercase")]
    pub current_modifier: Modifier,
    /// How the current_* styles are applied across the row.
    #[serde(deserialize_with = "camelcase_normalized")]
    pub row_connection_style: RowConnectionStyle,

    // status
    #[serde(deserialize_with = "camelcase_normalized")]
    pub status_fg: Color,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub status_modifier: Modifier,
    pub status_show: bool,

    // pub selected_fg: Color,
    // pub selected_bg: Color,
    // pub selected_modifier: Color,

    // scroll
    #[partial(alias = "c")]
    #[serde(alias = "cycle")]
    pub scroll_wrap: bool,
    #[partial(alias = "sp")]
    pub scroll_padding: u16,
    #[partial(alias = "r")]
    pub reverse: Option<bool>,

    // wrap
    #[partial(alias = "w")]
    pub wrap: bool,
    pub wrap_scaling_min_width: u16,

    // experimental
    pub column_spacing: Count,
    pub current_prefix: String,

    // lowpri: maybe space-around/space-between instead?
    #[partial(alias = "ra")]
    pub right_align_last: bool,

    #[partial(alias = "v")]
    #[serde(alias = "vertical")]
    pub stacked_columns: bool,

    #[serde(alias = "hr")]
    #[serde(deserialize_with = "camelcase_normalized")]
    pub horizontal_separator: HorizontalSeparator,
}
define_transparent_wrapper!(
    #[derive(Copy, Clone)]
    Count: u16 = 1
);

// #[derive(Default, Deserialize)]
// pub enum HorizontalSeperator {
//     None,

// }

impl Default for ResultsConfig {
    fn default() -> Self {
        ResultsConfig {
            border: Default::default(),

            multi_prefix: "▌ ".to_string(),
            default_prefix: Default::default(),

            fg: Default::default(),
            modifier: Default::default(),
            match_fg: Color::Green,
            match_modifier: Modifier::ITALIC,

            current_fg: Default::default(),
            current_bg: Color::Black,
            current_modifier: Modifier::BOLD,
            row_connection_style: RowConnectionStyle::Disjoint,

            status_fg: Color::Green,
            status_modifier: Modifier::ITALIC,
            status_show: true,

            scroll_wrap: true,
            scroll_padding: 2,
            reverse: Default::default(),

            wrap: Default::default(),
            wrap_scaling_min_width: 5,

            column_spacing: Default::default(),
            current_prefix: Default::default(),
            right_align_last: false,
            stacked_columns: false,
            horizontal_separator: Default::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Deserialize))]
pub struct DisplayConfig {
    #[partial(recurse)]
    pub border: BorderSetting,

    #[serde(deserialize_with = "camelcase_normalized")]
    pub fg: Color,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub modifier: Modifier,

    pub match_indent: bool,
    pub wrap: bool,

    #[serde(deserialize_with = "deserialize_option_auto")]
    pub content: Option<StringOrVec>,

    /// This setting controls the effective width of the displayed content.
    /// - Full: Effective width is the full ui width.
    /// - Capped: Effective width is the full ui width, but
    ///   any width exceeding the width of the Results UI is occluded by the preview pane.
    /// - Disjoint: Effective width is same as the Results UI.
    ///
    /// # Note
    /// The width effect only applies on the footer, and when the content is singular.
    #[serde(deserialize_with = "camelcase_normalized")]
    pub row_connection_style: RowConnectionStyle,

    /// This setting controls how many lines are read from the input for display with the header.
    ///
    /// # Note
    /// This only affects the header and is only implemented in the binary.
    #[partial(alias = "h")]
    pub header_lines: usize,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        DisplayConfig {
            border: Default::default(),
            match_indent: true,
            fg: Color::Green,
            wrap: false,
            row_connection_style: Default::default(),
            modifier: Modifier::ITALIC, // whatever your `deserialize_modifier` default uses
            content: None,
            header_lines: 0,
        }
    }
}

/// # Example
/// ```rust
/// use matchmaker::config::{PreviewConfig, PreviewSetting, PreviewLayout};
///
/// let _ = PreviewConfig {
///     layout: vec![
///         PreviewSetting {
///             layout: PreviewLayout::default(),
///             command: String::new()
///         }
///     ],
///     ..Default::default()
/// };
/// ```
#[partial(path, derive(Debug, Deserialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewConfig {
    #[partial(recurse)]
    pub border: BorderSetting,
    #[partial(recurse)]
    #[partial(alias = "l")]
    pub layout: Vec<PreviewSetting>,
    #[partial(recurse)]
    #[serde(flatten)]
    pub scroll: PreviewScrollSetting,
    /// Whether to cycle to top after scrolling to the bottom and vice versa.
    #[partial(alias = "c")]
    #[serde(alias = "cycle")]
    pub scroll_wrap: bool,
    pub wrap: bool,
    pub show: bool,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        PreviewConfig {
            border: BorderSetting {
                padding: Padding::left(2),
                ..Default::default()
            },
            scroll: Default::default(),
            layout: Default::default(),
            scroll_wrap: true,
            wrap: Default::default(),
            show: Default::default(),
        }
    }
}

/// Determines the initial scroll offset of the preview window.
#[partial(path, derive(Debug, Deserialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct PreviewScrollSetting {
    /// Extract the initial display index `n` of the preview window from this column.
    /// `n` lines are skipped after the header lines are consumed.
    pub index: Option<String>,
    /// For adjusting the initial scroll index.
    #[partial(alias = "o")]
    pub offset: isize,
    /// How far from the bottom of the preview window the scroll offset should appear.
    #[partial(alias = "p")]
    pub percentage: Percentage,
    /// Keep the top N lines as the fixed header so that they are always visible.
    #[partial(alias = "h")]
    pub header_lines: usize,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewerConfig {
    pub try_lossy: bool,

    // todo
    pub cache: u8,

    pub help_colors: TomlColorConfig,
}

/// Help coloring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TomlColorConfig {
    #[serde(deserialize_with = "camelcase_normalized")]
    pub section: Color,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub key: Color,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub string: Color,
    #[serde(deserialize_with = "camelcase_normalized")]
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

#[derive(Default, Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Deserialize))]
pub struct BorderSetting {
    #[serde(with = "through_string")]
    pub r#type: BorderType,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub color: Color,
    /// Given as sides joined by `|`. i.e.:
    /// `sides = "TOP | BOTTOM"``
    /// When omitted, this is ALL if either padding or type are specified, otherwose NONE.
    ///
    /// An empty string enforces no sides:
    /// `sides = ""`
    // #[serde(deserialize_with = "uppercase_normalized_option")] // need ratatui bitflags to use transparent
    pub sides: Option<Borders>,
    /// Supply as either 1, 2, or 4 numbers for:
    ///
    /// - Same padding on all sides
    /// - Vertical and horizontal padding values
    /// - Top, Right, Bottom, Left padding values
    ///
    /// respectively.
    #[serde(with = "padding")]
    pub padding: Padding,
    pub title: String,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub title_modifier: Modifier,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub bg: Color,
}

impl BorderSetting {
    pub fn as_block(&self) -> ratatui::widgets::Block<'_> {
        let mut ret = ratatui::widgets::Block::default()
            .padding(self.padding)
            .style(Style::default().bg(self.bg));

        if !self.title.is_empty() {
            let title = Span::styled(
                &self.title,
                Style::default().add_modifier(self.title_modifier),
            );

            ret = ret.title(title)
        };

        if !self.is_empty() {
            ret = ret
                .borders(self.sides())
                .border_type(self.r#type)
                .border_style(ratatui::style::Style::default().fg(self.color))
        }

        ret
    }

    pub fn sides(&self) -> Borders {
        if let Some(s) = self.sides {
            s
        } else if self.color != Default::default() || self.r#type != Default::default() {
            Borders::ALL
        } else {
            Borders::NONE
        }
    }

    pub fn as_static_block(&self) -> ratatui::widgets::Block<'static> {
        let mut ret = ratatui::widgets::Block::default()
            .padding(self.padding)
            .style(Style::default().bg(self.bg));

        if !self.title.is_empty() {
            let title: Span<'static> = Span::styled(
                self.title.clone(),
                Style::default().add_modifier(self.title_modifier),
            );

            ret = ret.title(title)
        };

        if !self.is_empty() {
            ret = ret
                .borders(self.sides())
                .border_type(self.r#type)
                .border_style(ratatui::style::Style::default().fg(self.color))
        }

        ret
    }

    pub fn is_empty(&self) -> bool {
        self.sides() == Borders::NONE
    }

    pub fn height(&self) -> u16 {
        let mut height = 0;
        height += 2 * !self.is_empty() as u16;
        height += self.padding.top + self.padding.bottom;
        height += (!self.title.is_empty() as u16).saturating_sub(!self.is_empty() as u16);

        height
    }

    pub fn width(&self) -> u16 {
        let mut width = 0;
        width += 2 * !self.is_empty() as u16;
        width += self.padding.left + self.padding.right;

        width
    }

    pub fn left(&self) -> u16 {
        let mut width = 0;
        width += !self.is_empty() as u16;
        width += self.padding.left;

        width
    }

    pub fn top(&self) -> u16 {
        let mut height = 0;
        height += !self.is_empty() as u16;
        height += self.padding.top;
        height += (!self.title.is_empty() as u16).saturating_sub(!self.is_empty() as u16);

        height
    }
}

// how to determine how many rows to allocate?
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[partial(path, derive(Debug, Deserialize))]
pub struct TerminalLayoutSettings {
    /// Percentage of total rows to occupy.
    #[partial(alias = "p")]
    pub percentage: Percentage,
    pub min: u16,
    pub max: u16, // 0 for terminal height cap
}

impl Default for TerminalLayoutSettings {
    fn default() -> Self {
        Self {
            percentage: Percentage::new(50),
            min: 10,
            max: 120,
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

#[partial(path, derive(Debug, Deserialize))]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewSetting {
    #[serde(flatten)]
    #[partial(recurse)]
    pub layout: PreviewLayout,
    #[serde(default, alias = "cmd", alias = "x")]
    pub command: String,
}

#[partial(path, derive(Debug, Deserialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewLayout {
    pub side: Side,
    /// Percentage of total rows/columns to occupy.

    #[serde(alias = "p")]
    // we need serde here since its specified inside the value but i don't think there's another case for it.
    pub percentage: Percentage,
    pub min: i16,
    pub max: i16,
}

impl Default for PreviewLayout {
    fn default() -> Self {
        Self {
            side: Side::Right,
            percentage: Percentage::new(60),
            min: 30,
            max: 120,
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

use crate::utils::serde::bounded_usize;
// todo: pass filter and hidden to mm
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Deserialize))]
pub struct ColumnsConfig {
    /// The strategy of how columns are parsed from input lines
    pub split: Split,
    /// Column names
    #[partial(alias = "n")]
    pub names: Vec<ColumnSetting>,
    /// Maximum number of columns to autogenerate when names is unspecified. Maximum of 10, minimum of 1.
    #[serde(deserialize_with = "bounded_usize::<_, 1, {crate::MAX_SPLITS}>")]
    max_columns: usize,
}

impl ColumnsConfig {
    pub fn max_cols(&self) -> usize {
        self.max_columns.min(MAX_SPLITS).max(1)
    }
}

impl Default for ColumnsConfig {
    fn default() -> Self {
        Self {
            split: Default::default(),
            names: Default::default(),
            max_columns: 5,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct ColumnSetting {
    pub filter: bool,
    pub hidden: bool,
    pub name: String,
}

#[derive(Default, Debug, Clone)]
pub enum Split {
    /// Split by delimiter. Supports regex.
    Delimiter(Regex),
    /// A sequence of regexes.
    Regexes(Vec<Regex>),
    /// No splitting.
    #[default]
    None,
}

impl PartialEq for Split {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Split::Delimiter(r1), Split::Delimiter(r2)) => r1.as_str() == r2.as_str(),
            (Split::Regexes(v1), Split::Regexes(v2)) => {
                if v1.len() != v2.len() {
                    return false;
                }
                v1.iter()
                    .zip(v2.iter())
                    .all(|(r1, r2)| r1.as_str() == r2.as_str())
            }
            (Split::None, Split::None) => true,
            _ => false,
        }
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

pub fn deserialize_string_or_char_as_double_width<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: From<String>,
{
    struct GenericVisitor<T> {
        _marker: std::marker::PhantomData<T>,
    }

    impl<'de, T> Visitor<'de> for GenericVisitor<T>
    where
        T: From<String>,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or single character")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let s = if v.chars().count() == 1 {
                let mut s = String::with_capacity(2);
                s.push(v.chars().next().unwrap());
                s.push(' ');
                s
            } else {
                v.to_string()
            };
            Ok(T::from(s))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&v)
        }
    }

    deserializer.deserialize_string(GenericVisitor {
        _marker: std::marker::PhantomData,
    })
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
                    let r = Regex::new(&s)
                        .map_err(|e| de::Error::custom(format!("Invalid regex: {}", e)))?;
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

        fn default_true() -> bool {
            true
        }

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

mod padding {
    use super::*;

    pub fn serialize<S>(padding: &Padding, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        if padding.top == padding.bottom
            && padding.left == padding.right
            && padding.top == padding.left
        {
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

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Padding, D::Error>
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
                let v = u16::try_from(value).map_err(|_| {
                    E::custom(format!("padding value {} is out of range for u16", value))
                })?;

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
                let v = u16::try_from(value).map_err(|_| {
                    E::custom(format!("padding value {} is out of range for u16", value))
                })?;

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
}

pub fn deserialize_option_auto<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt.as_deref() {
        Some("auto") => Ok(None),
        Some(s) => Ok(Some(T::deserialize(s.into_deserializer())?)),
        None => Ok(None),
    }
}
