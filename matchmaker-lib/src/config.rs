//! Config Types.
//! See `src/bin/mm/config.rs` for an example

use matchmaker_partial_macros::partial;

pub use crate::config_types::*;
pub use crate::utils::{Percentage, serde::StringOrVec};

use crate::{
    MAX_SPLITS,
    tui::IoStream,
    utils::serde::{escaped_opt_char, escaped_opt_string, serde_duration_ms},
};

use cli_boilerplate_automation::serde::transform::{
    camelcase_normalized, camelcase_normalized_option,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{BorderType, Borders},
};

use serde::{Deserialize, Serialize};

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
}

/// "Input/output specific". Configures the matchmaker worker.
///
/// Does not deny unknown fields.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct WorkerConfig {
    #[partial(recurse)]
    #[serde(flatten)]
    /// How columns are parsed from input lines
    pub columns: ColumnsConfig,
    /// How "stable" the results are. Higher values prioritize the initial ordering.
    pub sort_threshold: u32,

    /// TODO: Enable raw mode where non-matching items are also displayed in a dimmed color.
    #[partial(alias = "r")]
    pub raw: bool,
    /// TODO: Track the current selection when the result list is updated.
    pub track: bool,
    /// Reverse the order of the input
    pub reverse: bool, // TODO: test with sort_threshold
}

/// Configures how input is fed to to the worker(s).
///
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct StartConfig {
    #[serde(deserialize_with = "escaped_opt_char")]
    #[partial(alias = "is")]
    pub input_separator: Option<char>,
    #[serde(deserialize_with = "escaped_opt_string")]
    #[partial(alias = "os")]
    pub output_separator: Option<String>,

    /// Format string to print accepted items as.
    #[partial(alias = "ot")]
    #[serde(alias = "output")]
    pub output_template: Option<String>,

    /// Default command to execute when stdin is not being read.
    #[partial(alias = "cmd", alias = "x")]
    pub command: String,
    pub sync: bool,

    /// Whether to parse ansi sequences from input
    #[partial(alias = "a")]
    pub ansi: bool,
    /// Trim the input
    #[partial(alias = "t")]
    pub trim: bool,
}

/// Exit conditions of the render loop.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct ExitConfig {
    /// Exit automatically if there is only one match.
    pub select_1: bool,
    /// Allow returning without any items selected.
    pub allow_empty: bool,
    /// Abort if no items.
    pub abort_empty: bool,
    /// Last processed key is written here.
    /// Set to an empty path to disable.
    pub last_key_path: Option<std::path::PathBuf>,
}

/// The ui config.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(recurse, path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct RenderConfig {
    /// The default overlay style
    pub ui: UiConfig,
    /// The input bar style
    #[partial(alias = "i")]
    pub input: InputConfig,
    /// The results table style
    #[partial(alias = "r")]
    pub results: ResultsConfig,

    /// The results status style
    pub status: StatusConfig,
    /// The preview panel style
    #[partial(alias = "p")]
    pub preview: PreviewConfig,
    #[partial(alias = "f")]
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
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
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

/// The container ui.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
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
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
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

    /// The prompt prefix.
    #[serde(deserialize_with = "deserialize_string_or_char_as_double_width")]
    pub prompt: String,
    /// Cursor style.
    pub cursor: CursorSetting,

    /// Initial text in the input bar.
    #[partial(alias = "i")]
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
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct OverlayConfig {
    #[partial(recurse)]
    pub border: BorderSetting,
    pub outer_dim: bool,
    pub layout: OverlayLayoutSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct OverlayLayoutSettings {
    /// w, h
    #[partial(alias = "p")]
    pub percentage: [Percentage; 2],
    /// w, h
    pub min: [u16; 2],
    /// w, h
    pub max: [u16; 2],

    /// y_offset as a percentage of total height: 50 for neutral, (default: 55)
    pub y_offset: Percentage,
}

impl Default for OverlayLayoutSettings {
    fn default() -> Self {
        Self {
            percentage: [Percentage::new(60), Percentage::new(30)],
            min: [10, 10],
            max: [200, 30],
            y_offset: Percentage::new(55),
        }
    }
}

// pub struct OverlaySize

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResultsConfig {
    #[partial(recurse)]
    pub border: BorderSetting,

    // prefixes
    #[serde(deserialize_with = "deserialize_string_or_char_as_double_width")]
    pub multi_prefix: String,
    pub default_prefix: String,

    /// Enable selections
    pub multi: bool,

    // text styles
    #[serde(deserialize_with = "camelcase_normalized")]
    pub fg: Color,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub bg: Color,
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
    pub min_wrap_width: u16,

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
            multi: true,

            fg: Default::default(),
            modifier: Default::default(),
            bg: Default::default(),
            match_fg: Color::Green,
            match_modifier: Modifier::ITALIC,

            current_fg: Default::default(),
            current_bg: Color::Black,
            current_modifier: Modifier::BOLD,
            row_connection_style: RowConnectionStyle::Disjoint,

            scroll_wrap: true,
            scroll_padding: 2,
            reverse: Default::default(),

            wrap: Default::default(),
            min_wrap_width: 6,

            column_spacing: Default::default(),
            current_prefix: Default::default(),
            right_align_last: false,
            stacked_columns: false,
            horizontal_separator: Default::default(),
        }
    }
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatusConfig {
    #[serde(deserialize_with = "camelcase_normalized")]
    pub fg: Color,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub modifier: Modifier,
    /// Whether the status is visible.
    pub show: bool,
    /// Indent the status to match the results.
    pub match_indent: bool,

    /// Supports replacements:
    /// - `\r` -> cursor index
    /// - `\m` -> match count
    /// - `\t` -> total count
    /// - `\s` -> available whitespace / # appearances
    #[partial(alias = "t")]
    pub template: String,

    /// - Full: available whitespace is computed using the full ui width when replacing `\s` in the template.
    /// - Disjoint: no effect.
    /// - Capped: no effect.
    pub row_connection_style: RowConnectionStyle,
}
impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            fg: Color::Green,
            modifier: Modifier::ITALIC,
            show: true,
            match_indent: true,
            template: r#"\m/\t"#.to_string(),
            row_connection_style: RowConnectionStyle::Full,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct DisplayConfig {
    #[partial(recurse)]
    pub border: BorderSetting,

    #[serde(deserialize_with = "camelcase_normalized")]
    pub fg: Color,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub modifier: Modifier,

    /// Indent content to match the results table.
    pub match_indent: bool,
    /// Enable line wrapping.
    pub wrap: bool,

    /// Static content to display.
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
///             command: String::new(),
///             ..Default::default()
///         }
///     ],
///     ..Default::default()
/// };
/// ```
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewConfig {
    #[partial(recurse)]
    pub border: BorderSetting,
    #[partial(recurse, set = "recurse")]
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
    /// Whether to show the preview pane initially.
    /// Can either be a boolean or a number which the relevant dimension of the available ui area must exceed.
    pub show: ShowCondition,

    pub reevaluate_show_on_resize: bool,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        PreviewConfig {
            border: BorderSetting {
                padding: Padding(ratatui::widgets::Padding::left(2)),
                ..Default::default()
            },
            scroll: Default::default(),
            layout: Default::default(),
            scroll_wrap: true,
            wrap: Default::default(),
            show: Default::default(),
            reevaluate_show_on_resize: false,
        }
    }
}

/// Determines the initial scroll offset of the preview window.
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

impl Default for PreviewScrollSetting {
    fn default() -> Self {
        Self {
            index: Default::default(),
            offset: -1,
            percentage: Default::default(),
            header_lines: Default::default(),
        }
    }
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

#[derive(Default, Debug, Clone, PartialEq, Deserialize, Serialize)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct BorderSetting {
    #[serde(deserialize_with = "camelcase_normalized_option")]
    pub r#type: Option<BorderType>,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub color: Color,
    /// Given as sides joined by `|`. i.e.:
    /// `sides = "TOP | BOTTOM"``
    /// `sides = "ALL"`
    /// When omitted, this either ALL or the side that sits between results and the corresponding layout if either padding or type are specified, otherwise NONE.
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
    pub padding: Padding,
    pub title: String,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub title_modifier: Modifier,
    pub modifier: Modifier,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub bg: Color,
}

impl BorderSetting {
    pub fn as_block(&self) -> ratatui::widgets::Block<'_> {
        let mut ret = ratatui::widgets::Block::default()
            .padding(self.padding.0)
            .style(Style::default().bg(self.bg).add_modifier(self.modifier));

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
                .border_type(self.r#type.unwrap_or_default())
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
            .padding(self.padding.0)
            .style(Style::default().bg(self.bg).add_modifier(self.modifier));

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
                .border_type(self.r#type.unwrap_or_default())
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
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
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

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewSetting {
    #[serde(flatten)]
    #[partial(recurse)]
    pub layout: PreviewLayout,
    #[partial(recurse)]
    pub border: Option<BorderSetting>,
    #[serde(default, alias = "cmd", alias = "x")]
    pub command: String,
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
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

use crate::utils::serde::bounded_usize;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct ColumnsConfig {
    /// The strategy of how columns are parsed from input lines
    #[partial(alias = "s")]
    pub split: Split,
    /// Column names
    #[partial(alias = "n")]
    pub names: Vec<ColumnSetting>,
    /// Maximum number of columns to autogenerate when names is unspecified. Maximum of 16, minimum of 1.
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
            max_columns: 6,
        }
    }
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
