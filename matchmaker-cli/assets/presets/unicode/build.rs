#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! anyhow = "1"
//! clap = { version = "4", features = ["derive"] }
//! reqwest = { version = "0.12", features = ["blocking"] }
//! serde = { version = "1", features = ["derive"] }
//! serde_json = "1"
//! unicode-properties = "0.1"
//! unicode-segmentation = "1"
//! unicode-width = "0.2"
//! zstd = "0.13"
//! ```

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use serde_json::Value;
use unicode_properties::{
    EmojiStatus, GeneralCategoryGroup, UnicodeEmoji, UnicodeGeneralCategory,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const EMOJI_TEST_URL: &str = "https://www.unicode.org/Public/emoji/latest/emoji-test.txt";
const CLDR_ANNOTATIONS_BASE: &str =
    "https://raw.githubusercontent.com/unicode-org/cldr-json/main/cldr-json/cldr-annotations-full/annotations";
const CLDR_DERIVED_BASE: &str =
    "https://raw.githubusercontent.com/unicode-org/cldr-json/main/cldr-json/cldr-annotations-derived-full/annotationsDerived";
const UNICODE_DATA_URL: &str = "https://www.unicode.org/Public/UCD/latest/ucd/UnicodeData.txt";
const PROP_ALIASES_URL: &str =
    "https://www.unicode.org/Public/UCD/latest/ucd/PropertyValueAliases.txt";
const LATEX_MAP_URL: &str =
    "https://raw.githubusercontent.com/latex3/unicode-math/master/unicode-math-table.tex";
const HTML_ENTITIES_URL: &str = "https://html.spec.whatwg.org/entities.json";
const NERD_FONTS_URL: &str =
    "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/master/glyphnames.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tier {
    Strict,
    Modern,
    Full,
}

#[derive(Parser, Debug)]
#[command(
    name = "gen-unicode-data",
    about = "Generate 4-column [char, name, cat, meta] emoji.zst and unicode.zst datasets"
)]
struct Cli {
    /// Strict mode: single-codepoint emojis (<= E13.0), no Ambiguous width or text-default emojis
    #[arg(long, group = "tier")]
    strict: bool,

    /// Modern mode (default): DEC Mode 2027 single-grapheme emojis (<= E15.1), safe symbols
    #[arg(long, group = "tier")]
    modern: bool,

    /// Full mode: all fully-qualified emojis and Unicode symbols (for patched Alacritty + ratatui)
    #[arg(long, group = "tier")]
    full: bool,

    /// Include Nerd Font PUA glyphs from ryanoasis/nerd-fonts in unicode.zst
    #[arg(long)]
    nerd_fonts: bool,

    /// Skip downloading and embedding CLDR keyword annotations
    #[arg(long)]
    no_cldr: bool,

    /// CLDR locale for keyword annotations (e.g. en, de, fr, ja, zh-Hans)
    #[arg(long, default_value = "en")]
    locale: String,

    /// Output directory for emoji.zst and unicode.zst
    #[arg(short, long, default_value = ".")]
    out_dir: PathBuf,

    /// Cache directory for downloaded upstream files
    #[arg(short, long, default_value = ".cache")]
    cache_dir: PathBuf,

    /// Force re-downloading upstream files
    #[arg(long)]
    refresh: bool,

    /// Write uncompressed .tsv files instead of .zst
    #[arg(long)]
    no_zstd: bool,

    /// Zstd compression level (1..=22)
    #[arg(long, default_value_t = 19)]
    zstd_level: i32,
}

impl Cli {
    fn tier(&self) -> Tier {
        if self.strict {
            Tier::Strict
        } else if self.full {
            Tier::Full
        } else {
            Tier::Modern
        }
    }
}

#[derive(Deserialize)]
struct HtmlEntityEntry {
    codepoints: Vec<u32>,
}

#[derive(Deserialize)]
struct CldrRoot {
    #[serde(alias = "annotationsDerived")]
    annotations: CldrInner,
}

#[derive(Deserialize)]
struct CldrInner {
    annotations: HashMap<String, CldrEntry>,
}

#[derive(Deserialize)]
struct CldrEntry {
    #[serde(default)]
    default: Vec<String>,
}

fn fetch_cached(url: &str, filename: &str, cache_dir: &Path, refresh: bool) -> Result<String> {
    fs::create_dir_all(cache_dir)?;
    let path = cache_dir.join(filename);

    if !path.exists() || refresh {
        eprintln!("Downloading {url} -> {}", path.display());
        let text = reqwest::blocking::get(url)
            .with_context(|| format!("Failed to GET {url}"))?
            .error_for_status()
            .with_context(|| format!("HTTP error for {url}"))?
            .text()?;
        fs::write(&path, &text)?;
        Ok(text)
    } else {
        eprintln!("Using cached {}", path.display());
        Ok(fs::read_to_string(&path)?)
    }
}

fn write_dataset(
    out_dir: &Path,
    stem: &str,
    lines: &[String],
    no_zstd: bool,
    zstd_level: i32,
) -> Result<()> {
    fs::create_dir_all(out_dir)?;
    let mut payload = lines.join("\n");
    if !payload.is_empty() {
        payload.push('\n');
    }

    let ext = if no_zstd { "tsv" } else { "zst" };
    let out_path = out_dir.join(format!("{stem}.{ext}"));

    if no_zstd {
        fs::write(&out_path, payload)?;
    } else {
        let compressed = zstd::stream::encode_all(Cursor::new(payload.as_bytes()), zstd_level)?;
        fs::write(&out_path, compressed)?;
    }
    eprintln!("Wrote {} ({} entries)", out_path.display(), lines.len());
    Ok(())
}

// ---------------------------------------------------------------------------
// CLDR Annotations & Shared Meta Formatting
// ---------------------------------------------------------------------------

fn load_cldr(
    cache_dir: &Path,
    refresh: bool,
    no_cldr: bool,
    locale: &str,
) -> Result<HashMap<String, Vec<String>>> {
    let mut map = HashMap::new();
    if no_cldr {
        return Ok(map);
    }

    let locale = locale.trim().replace('_', "-");
    let annotations_url = format!("{CLDR_ANNOTATIONS_BASE}/{locale}/annotations.json");
    let derived_url = format!("{CLDR_DERIVED_BASE}/{locale}/annotations.json");
    let annotations_file = format!("cldr-{locale}-annotations.json");
    let derived_file = format!("cldr-{locale}-annotations-derived.json");

    for (url, file) in [
        (annotations_url, annotations_file),
        (derived_url, derived_file),
    ] {
        let raw = fetch_cached(&url, &file, cache_dir, refresh)?;
        let parsed: CldrRoot =
            serde_json::from_str(&raw).with_context(|| format!("Failed to parse {file}"))?;
        for (glyph, entry) in parsed.annotations.annotations {
            map.entry(glyph).or_default().extend(entry.default);
        }
    }
    Ok(map)
}

fn format_meta(
    primary: Option<&str>,
    cldr_map: &HashMap<String, Vec<String>>,
    glyph: &str,
    name: &str,
) -> String {
    let stripped = glyph.replace('\u{FE0F}', "");
    let keywords = cldr_map
        .get(glyph)
        .or_else(|| cldr_map.get(&stripped))
        .map(|list| {
            let name_lower = name.to_lowercase();
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for kw in list {
                let cleaned = kw.replace(':', "");
                let trimmed = cleaned.trim();
                let lower = trimmed.to_lowercase();
                if !lower.is_empty() && lower != name_lower && seen.insert(lower) {
                    out.push(trimmed.to_string());
                }
            }
            out.join(" ")
        })
        .unwrap_or_default();

    match (primary.filter(|s| !s.is_empty()), keywords.is_empty()) {
        (Some(p), false) => format!("{p} | {keywords}"),
        (Some(p), true) => p.to_string(),
        (None, false) => keywords,
        (None, true) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 1. Emoji Generation: [char, name, cat (group), meta (subgroup | cldr)]
// ---------------------------------------------------------------------------

fn allow_emoji(
    emoji: &str,
    codepoints: &[u32],
    status: &str,
    emoji_version: f32,
    tier: Tier,
) -> bool {
    if status != "fully-qualified" {
        return false;
    }

    let max_version = match tier {
        Tier::Strict => 13.0,
        Tier::Modern => 15.1,
        Tier::Full => 99.0,
    };
    if emoji_version > max_version {
        return false;
    }

    let width = UnicodeWidthStr::width(emoji);
    match tier {
        Tier::Strict => codepoints.len() == 1 && width == 2,
        Tier::Modern => {
            !codepoints
                .iter()
                .any(|&cp| (0xE0020..=0xE007F).contains(&cp) || cp == 0xFE0E)
                && UnicodeSegmentation::graphemes(emoji, true).count() == 1
                && width == 2
        }
        Tier::Full => width == 1 || width == 2,
    }
}

fn build_emojis(
    raw: &str,
    cldr_map: &HashMap<String, Vec<String>>,
    tier: Tier,
) -> (Vec<String>, HashSet<u32>) {
    let mut lines = Vec::new();
    let mut seen_single_cps = HashSet::new();
    let mut group = "Uncategorized";
    let mut subgroup = "other";

    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(g) = trimmed.strip_prefix("# group:") {
            group = g.trim();
            continue;
        }
        if let Some(sg) = trimmed.strip_prefix("# subgroup:") {
            subgroup = sg.trim();
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((lhs, rhs)) = trimmed.split_once(';') else {
            continue;
        };
        let Some((status_part, comment_part)) = rhs.split_once('#') else {
            continue;
        };

        let codepoints: Vec<u32> = lhs
            .split_whitespace()
            .filter_map(|hex| u32::from_str_radix(hex, 16).ok())
            .collect();
        if codepoints.is_empty() {
            continue;
        }

        let emoji: String = codepoints.iter().filter_map(|&cp| char::from_u32(cp)).collect();

        let mut tokens = comment_part.trim().splitn(3, ' ');
        let _glyph = tokens.next();
        let emoji_version = tokens
            .next()
            .and_then(|t| t.strip_prefix('E'))
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        let name = tokens.next().unwrap_or("").trim();

        if name.is_empty() || !allow_emoji(&emoji, &codepoints, status_part.trim(), emoji_version, tier) {
            continue;
        }

        if codepoints.len() == 1 {
            seen_single_cps.insert(codepoints[0]);
        }

        let meta = format_meta(Some(subgroup), cldr_map, &emoji, name);
        lines.push(format!("{emoji}\t{name}\t{group}\t{meta}"));
    }

    (lines, seen_single_cps)
}

// ---------------------------------------------------------------------------
// 2. Unicode + Nerd Fonts: [char, name, cat (official gc), meta]
// ---------------------------------------------------------------------------

fn parse_gc_aliases(raw: &str) -> HashMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split(';').map(str::trim);
            (parts.next() == Some("gc"))
                .then(|| Some((parts.next()?.to_string(), parts.next()?.to_string())))
                .flatten()
        })
        .collect()
}

fn parse_latex_map(raw: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for line in raw.lines() {
        let Some((_, rest)) = line.split_once("\\UnicodeMathSymbol{") else {
            continue;
        };
        let mut parts = rest.split('{');
        let Some(hex_raw) = parts.next() else {
            continue;
        };
        let Some(cmd_raw) = parts.next() else {
            continue;
        };
        let hex_str = hex_raw.trim_matches(|c| c == '"' || c == '}' || c == ' ');
        let cmd = cmd_raw.trim_matches(|c| c == '}' || c == ' ');
        if let Ok(cp) = u32::from_str_radix(hex_str, 16) {
            if !cmd.is_empty() {
                map.entry(cp).or_insert_with(|| cmd.to_string());
            }
        }
    }
    map
}

fn parse_html_entities(raw: &str) -> HashMap<u32, String> {
    let parsed: HashMap<String, HtmlEntityEntry> = serde_json::from_str(raw).unwrap_or_default();
    let mut map: HashMap<u32, String> = HashMap::new();

    for (entity, entry) in parsed {
        if entity.ends_with(';') && entry.codepoints.len() == 1 {
            map.entry(entry.codepoints[0])
                .and_modify(|existing| {
                    let key = (
                        entity.len(),
                        entity.chars().any(|c| c.is_ascii_uppercase()),
                        &entity,
                    );
                    let cur = (
                        existing.len(),
                        existing.chars().any(|c| c.is_ascii_uppercase()),
                        &*existing,
                    );
                    if key < cur {
                        *existing = entity.clone();
                    }
                })
                .or_insert(entity);
        }
    }
    map
}

fn allow_unicode_char(ch: char, bidi_class: &str, tier: Tier) -> bool {
    if !matches!(
        ch.general_category_group(),
        GeneralCategoryGroup::Letter
            | GeneralCategoryGroup::Number
            | GeneralCategoryGroup::Punctuation
            | GeneralCategoryGroup::Symbol
    ) {
        return false;
    }

    if matches!(
        bidi_class,
        "R" | "AL" | "RLE" | "LRE" | "RLO" | "LRO" | "PDF" | "RLI" | "LRI" | "FSI" | "PDI"
    ) {
        return false;
    }

    if !matches!(UnicodeWidthChar::width(ch), Some(1 | 2)) {
        return false;
    }

    match tier {
        Tier::Strict => !ch.is_emoji_char() && ch.width_cjk() == ch.width(),
        Tier::Modern => !ch.is_emoji_char() || ch.emoji_status() == EmojiStatus::EmojiPresentation,
        Tier::Full => true,
    }
}

fn build_unicode(
    raw: &str,
    gc_aliases: &HashMap<String, String>,
    latex_map: &HashMap<u32, String>,
    html_map: &HashMap<u32, String>,
    cldr_map: &HashMap<String, Vec<String>>,
    emoji_cps: &HashSet<u32>,
    tier: Tier,
) -> (Vec<String>, HashSet<u32>) {
    let mut lines = Vec::new();
    let mut seen_cps = emoji_cps.clone();
    let mut glyph_buf = [0u8; 4];

    for line in raw.lines() {
        let mut parts = line.split(';').map(str::trim);
        let (Some(hex_str), Some(name), Some(short_cat), _, Some(bidi_class)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };

        if name.is_empty() || (name.starts_with('<') && name.ends_with('>')) {
            continue;
        }
        let Ok(cp) = u32::from_str_radix(hex_str, 16) else {
            continue;
        };
        if seen_cps.contains(&cp) {
            continue;
        }
        let Some(ch) = char::from_u32(cp) else {
            continue;
        };
        if !allow_unicode_char(ch, bidi_class, tier) {
            continue;
        }

        seen_cps.insert(cp);

        let cat = gc_aliases.get(short_cat).map(String::as_str).unwrap_or(short_cat);
        let code_meta = latex_map.get(&cp).or_else(|| html_map.get(&cp)).map(String::as_str);
        let meta = format_meta(code_meta, cldr_map, ch.encode_utf8(&mut glyph_buf), name);

        lines.push(format!("{ch}\t{name}\t{cat}\t{meta}"));
    }

    (lines, seen_cps)
}

fn append_nerd_fonts(
    raw: &str,
    seen_cps: &mut HashSet<u32>,
    lines: &mut Vec<String>,
) -> Result<()> {
    let parsed: HashMap<String, Value> =
        serde_json::from_str(raw).context("Failed to parse Nerd Fonts glyphnames.json")?;

    let mut entries: Vec<(String, u32, char)> = parsed
        .into_iter()
        .filter(|(k, _)| k != "METADATA")
        .filter_map(|(key, val)| {
            let hex = val.get("code")?.as_str()?;
            let cp = u32::from_str_radix(hex, 16).ok()?;
            let ch = char::from_u32(cp)?;
            (UnicodeWidthChar::width(ch).unwrap_or(0) > 0).then_some((key, cp, ch))
        })
        .collect();

    entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    for (key, cp, ch) in entries {
        if seen_cps.insert(cp) {
            let readable_name = key.replace('-', " ");
            lines.push(format!("{ch}\t{readable_name}\tNerd_Font\tnf-{key}"));
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let tier = cli.tier();
    eprintln!(
        "Generating datasets (tier: {tier:?}, cldr: {}, nerd_fonts: {})",
        if cli.no_cldr { "off" } else { &cli.locale },
        cli.nerd_fonts
    );

    let cldr_map = load_cldr(&cli.cache_dir, cli.refresh, cli.no_cldr, &cli.locale)?;

    let emoji_raw = fetch_cached(EMOJI_TEST_URL, "emoji-test.txt", &cli.cache_dir, cli.refresh)?;
    let (emoji_lines, emoji_cps) = build_emojis(&emoji_raw, &cldr_map, tier);
    write_dataset(&cli.out_dir, "emoji", &emoji_lines, cli.no_zstd, cli.zstd_level)?;

    let unicode_raw = fetch_cached(UNICODE_DATA_URL, "UnicodeData.txt", &cli.cache_dir, cli.refresh)?;
    let aliases_raw = fetch_cached(PROP_ALIASES_URL, "PropertyValueAliases.txt", &cli.cache_dir, cli.refresh)?;
    let latex_raw = fetch_cached(LATEX_MAP_URL, "unicode-math-table.tex", &cli.cache_dir, cli.refresh)?;
    let html_raw = fetch_cached(HTML_ENTITIES_URL, "entities.json", &cli.cache_dir, cli.refresh)?;

    let gc_aliases = parse_gc_aliases(&aliases_raw);
    let latex_map = parse_latex_map(&latex_raw);
    let html_map = parse_html_entities(&html_raw);
    let (mut unicode_lines, mut seen_cps) = build_unicode(
        &unicode_raw,
        &gc_aliases,
        &latex_map,
        &html_map,
        &cldr_map,
        &emoji_cps,
        tier,
    );

    if cli.nerd_fonts {
        let nf_raw = fetch_cached(NERD_FONTS_URL, "nerd-fonts-glyphnames.json", &cli.cache_dir, cli.refresh)?;
        append_nerd_fonts(&nf_raw, &mut seen_cps, &mut unicode_lines)?;
    }

    write_dataset(&cli.out_dir, "unicode", &unicode_lines, cli.no_zstd, cli.zstd_level)?;
    Ok(())
}