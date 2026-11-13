//! Markdown rendering shared by `mm --doc` and the preset docs behind `@help`.

use std::path::Path;

use matchmaker::{Text, config::StyleSetting, utils::style_to_crossterm};
use serde::{Deserialize, Serialize};
use termimad::{ATTRIBUTES, CompoundStyle, MadSkin, StyledChar, crossterm::style::ContentStyle};

/// The base palette. `Neutral` is termimad's background-agnostic default, the
/// other two shift the code colors toward a dark or light terminal background.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum MarkdownVariant {
    #[default]
    Neutral,
    Dark,
    Light,
}

/// How markdown is styled, both in `mm --doc` and in the preset docs paged by
/// `@help`. Every style defaults to no override, so the rendered look is
/// termimad's skin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MarkdownConfig {
    #[serde(deserialize_with = "cba::serde::transform::camelcase_normalized")]
    pub variant: MarkdownVariant,
    /// Bold text.
    pub bold: StyleSetting,
    /// Headings, every level.
    pub header: StyleSetting,
    pub inline_code: StyleSetting,
    /// Fenced code blocks.
    pub code_block: StyleSetting,
    /// List bullets.
    pub bullet: StyleSetting,
}

impl MarkdownConfig {
    /// Read the `[markdown]` table of a config file. Other tables are ignored,
    /// so an otherwise invalid config still renders docs with the defaults.
    pub fn load(path: &Path) -> Self {
        let Ok(src) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str::<MarkdownOnly>(&src)
            .map(|cfg| cfg.markdown)
            .unwrap_or_default()
    }

    pub fn skin(&self) -> MadSkin {
        let mut skin = match self.variant {
            MarkdownVariant::Neutral => MadSkin::default(),
            MarkdownVariant::Dark => MadSkin::default_dark(),
            MarkdownVariant::Light => MadSkin::default_light(),
        };
        patch(&mut skin.bold, self.bold);
        for header in &mut skin.headers {
            patch(&mut header.compound_style, self.header);
        }
        patch(&mut skin.inline_code, self.inline_code);
        patch(&mut skin.code_block.compound_style, self.code_block);
        let mut bullet = *skin.bullet.compound_style();
        patch(&mut bullet, self.bullet);
        skin.bullet = StyledChar::new(bullet, skin.bullet.nude_char());
        skin
    }

    /// Render markdown into styled terminal text. `width`, when known, is the
    /// budget mermaid diagrams are compacted to; prose is not reflowed.
    pub fn render(&self, md: &str, width: Option<usize>) -> Text<'static> {
        use ansi_to_tui::IntoText;
        let md = render_mermaid_fences(md, width);
        let ansi = self.skin().text(&md, None).to_string();
        ansi.into_text()
            .unwrap_or_else(|_| Text::raw(md.to_owned()))
    }
}

/// A fenced block opener: fence char, its length, the indentation, and what
/// follows the fence on the same line.
fn fence(line: &str) -> Option<(char, usize, usize, &str)> {
    let trimmed = line.trim_start_matches(' ');
    let indent = line.len() - trimmed.len();
    let ch = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = trimmed.chars().take_while(|c| *c == ch).count();
    (len >= 3).then(|| (ch, len, indent, &trimmed[len..]))
}

/// Replace ```mermaid fenced blocks with the box diagram they describe. Blocks
/// that fail to render are left in place, so a broken diagram still reads as
/// code instead of vanishing.
fn render_mermaid_fences(md: &str, width: Option<usize>) -> String {
    let mut out = String::with_capacity(md.len());
    let mut lines = md.lines();
    while let Some(line) = lines.next() {
        let Some((ch, len, indent, info)) = fence(line) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let closing = |l: &str| {
            fence(l).is_some_and(|(c, n, _, rest)| c == ch && n >= len && rest.trim().is_empty())
        };
        let mut block = vec![line];
        let mut body = String::new();
        for next in lines.by_ref() {
            block.push(next);
            if closing(next) {
                break;
            }
            body.push_str(next.trim_start_matches(' '));
            body.push('\n');
        }
        let unclosed = !block.last().is_some_and(|l| closing(l));
        let diagram = if info.trim().eq_ignore_ascii_case("mermaid") && !unclosed {
            mermaid_text::render_with_width(&body, width)
                .ok()
                .filter(|d| !d.trim().is_empty())
        } else {
            None
        };
        match diagram {
            Some(diagram) => {
                let pad = " ".repeat(indent);
                out.push_str(&format!("{pad}```text\n"));
                for line in diagram.lines() {
                    out.push_str(&pad);
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str(&format!("{pad}```\n"));
            }
            None => {
                for line in block {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }
    out
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct MarkdownOnly {
    markdown: MarkdownConfig,
}

/// Apply the fields set in `style`, keeping termimad's defaults for the rest.
fn patch(compound: &mut CompoundStyle, style: StyleSetting) {
    let src: ContentStyle = style_to_crossterm(style.into_style_no_submodifiers());
    let dst = &mut compound.object_style;
    if src.foreground_color.is_some() {
        dst.foreground_color = src.foreground_color;
    }
    if src.background_color.is_some() {
        dst.background_color = src.background_color;
    }
    if src.underline_color.is_some() {
        dst.underline_color = src.underline_color;
    }
    for attr in ATTRIBUTES {
        if src.attributes.has(*attr) {
            dst.attributes.set(*attr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termimad::crossterm::style::{Attribute, Color as CColor};

    #[test]
    fn mermaid_fences_become_box_diagrams() {
        let md = "before\n\n```mermaid\ngraph LR; A[one] --> B[two]\n```\n\nafter\n";
        let out = render_mermaid_fences(md, Some(60));

        assert!(out.contains("```text"), "{out}");
        assert!(out.contains("│ one │"), "{out}");
        assert!(!out.contains("graph LR"), "{out}");
        assert!(out.starts_with("before\n") && out.ends_with("after\n"));
    }

    #[test]
    fn fences_that_are_not_diagrams_are_untouched() {
        // unknown mermaid syntax, a plain fence, and an unclosed fence
        let md = "```mermaid\nnope nope\n```\n```rust\nlet x = 1;\n```\n```mermaid\ngraph LR;\n";
        assert_eq!(render_mermaid_fences(md, Some(60)), md);
    }

    #[test]
    fn indented_fences_keep_their_indent() {
        let md = "  ```mermaid\n  graph LR; A[one] --> B[two]\n  ```\n";
        let out = render_mermaid_fences(md, Some(60));

        assert!(out.starts_with("  ```text\n"), "{out}");
        assert!(out.contains("\n  ┌"), "{out}");
    }

    #[test]
    fn styles_are_patched_over_the_skin() {
        let cfg: MarkdownConfig = toml::from_str(
            r##"
variant = "light"
bold.fg = "red"
header.modifier = "BOLD | ITALIC"
inline_code.bg = "#102030"
code_block.fg = "green"
bullet.fg = "cyan"
"##,
        )
        .unwrap();
        let skin = cfg.skin();
        let base = MadSkin::default_light();

        // set fields replace the base style, unset ones keep it
        assert_eq!(
            skin.bold.object_style.foreground_color,
            Some(CColor::DarkRed)
        );
        assert!(
            skin.bold.object_style.attributes.has(Attribute::Bold),
            "the base bold attribute survives an fg-only override"
        );
        assert_eq!(
            skin.code_block.compound_style.object_style.foreground_color,
            Some(CColor::DarkGreen)
        );
        assert_eq!(
            skin.code_block.compound_style.object_style.background_color,
            base.code_block.compound_style.object_style.background_color,
            "the base code block background survives an fg-only override"
        );
        assert!(
            skin.headers[0]
                .compound_style
                .object_style
                .attributes
                .has(Attribute::Bold),
            "termimad's header attributes survive the modifier patch"
        );
        assert!(
            skin.headers[4]
                .compound_style
                .object_style
                .attributes
                .has(Attribute::Italic)
        );
        assert_eq!(
            skin.inline_code.object_style.background_color,
            Some(CColor::Rgb {
                r: 0x10,
                g: 0x20,
                b: 0x30
            })
        );
        assert_eq!(
            skin.bullet.compound_style().object_style.foreground_color,
            Some(CColor::DarkCyan)
        );
        assert_eq!(skin.bullet.nude_char(), base.bullet.nude_char());
    }
}
