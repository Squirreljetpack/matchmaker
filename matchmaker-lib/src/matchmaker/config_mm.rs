use crate::{
    AcceptHook, Matchmaker,
    config::{
        ColumnsConfig, ExitConfig, PreprocessConfig, RenderConfig, Split, StringOrInt,
        TerminalConfig, WorkerConfig,
    },
    nucleo::{Column, Worker, injector::WorkerInjector},
    render::{EventHandlers, InterruptHandlers, MMState},
    utils::text::{self, sanitize_string},
};

use ansi_to_tui::IntoText;
use ratatui::text::Text;
use std::{borrow::Cow, sync::Arc};

pub struct OddEnds {
    pub hidden_columns: Vec<bool>,
    pub has_error: bool,
}

pub type ConfigMatchmaker = Matchmaker<String, String, ConfigPreprocessedData>;

impl ConfigMatchmaker {
    #[allow(unused)]
    /// Creates a new Matchmaker from a config::BaseConfig.
    /// Calls [`Matchmaker::prepare`];
    pub fn new_from_config(
        render_config: RenderConfig,
        tui_config: TerminalConfig,
        worker_config: WorkerConfig,
        columns_config: ColumnsConfig,
        exit_config: ExitConfig,
        preprocess_config: PreprocessConfig,
    ) -> (Self, ConfigInjector, OddEnds) {
        let mut has_error = false;

        let cc = columns_config;
        let hidden_columns = cc.names.iter().map(|x| x.hidden).collect();
        let offset = !cc.names_from_zero as usize;

        // Build column names
        let column_names: Vec<Arc<str>> = if cc.names.is_empty() {
            (offset..(cc.max_cols() + offset))
                .map(|n| Arc::from(n.to_string()))
                .collect()
        } else {
            cc.names
                .iter()
                .take(cc.max_cols())
                .map(|s| Arc::from(s.name.as_str()))
                .collect()
        };

        // Handle Split::None case - use empty name
        let (column_names, split) = match cc.split {
            Split::None => (vec![Arc::from("")], Split::None),
            _ => (column_names, cc.split),
        };

        // Find default column index
        let default_index = match cc.default {
            StringOrInt::String(ref s) => column_names
                .iter()
                .position(|name| name.as_ref() == s.as_str())
                .unwrap_or_else(|| {
                    cba::wbog!("Default column '{s}' not found, defaulting to first.");
                    0
                }),
            StringOrInt::Int(i) => {
                let i = i.saturating_sub(offset) as usize;
                if i < column_names.len() {
                    i
                } else {
                    cba::wbog!("Default column index {i} out of bounds, defaulting to first.");
                    0
                }
            }
        };

        // Build columns using the new function
        let (columns, raw_preprocessor, text_preprocessor) =
            build_columns(preprocess_config, split, column_names);

        let mut worker = Worker::new(columns, default_index, raw_preprocessor, text_preprocessor);

        #[cfg(feature = "experimental")]
        {
            worker.reverse_items(worker_config.reverse);
            worker.set_stability(*worker_config.sort_threshold);
            for (i, c) in cc.names.iter().enumerate() {
                worker.set_column_options(i, c.options)
            }
        }

        let injector = worker.injector();

        // Build the default accept_hook: returns empty Vec<String> by default.
        // The CLI overrides this in `start.rs` via the `on_accept`/`output_template`
        // logic. The hook is just a placeholder for the structural refactor; the
        // real accept pipeline will replace it.
        let accept_hook = Box::new(
            |_state: &mut MMState<'_, '_, String, ConfigPreprocessedData>| -> Vec<String> {
                vec![]
            },
        ) as AcceptHook<String, ConfigPreprocessedData, String>;

        let event_handlers = EventHandlers::new();
        let interrupt_handlers = InterruptHandlers::new();

        let mut new = Matchmaker {
            worker,
            render_config,
            tui_config,
            exit_config,
            output: accept_hook,
            event_handlers,
            interrupt_handlers,
        };
        new.prepare();

        let misc = OddEnds {
            hidden_columns,
            has_error,
        };

        (new, injector, misc)
    }
}

/// Preprocessed data type for config-based columns
/// Contains: (Result<Text, raw_string>, split_ranges)
pub type ConfigPreprocessedData = (Result<Text<'static>, String>, Vec<(u32, u32)>);
pub type ConfigInjector = WorkerInjector<String, ConfigPreprocessedData>;

/// Build columns for config-based matchmaker with preprocessing support.
/// Returns (columns, raw_preprocessor, text_preprocessor, default_column_index)
pub fn build_columns(
    PreprocessConfig {
        ansi,
        trim,
        sanitize,
        require_column,
    }: PreprocessConfig, // (parse_ansi, trim, skip_empty)
    split: crate::config::Split,
    column_names: Vec<Arc<str>>,
) -> (
    Vec<Column<String, ConfigPreprocessedData>>,
    Arc<dyn Fn(&String) -> Option<ConfigPreprocessedData> + Send + Sync>,
    Arc<dyn Fn(&String) -> ConfigPreprocessedData + Send + Sync>,
) {
    use crate::config::Split;
    use regex::Regex;

    let col_count = column_names.len();

    // Build split function based on config
    let split_fn: Arc<dyn Fn(&str) -> Vec<(u32, u32)> + Send + Sync> = match split {
        Split::Delimiter(ref rg) => {
            let rg = rg.clone();
            let column_names_clone = column_names.clone();
            let mut has_named_group = false;

            // Map named captures to column indices
            let capture_to_idx: Vec<Option<usize>> = rg
                .capture_names()
                .enumerate()
                .map(|(i, name_opt)| {
                    if i == 0 {
                        None
                    } else {
                        name_opt.and_then(|name| {
                            has_named_group = true;
                            column_names_clone.iter().position(|n| n.as_ref() == name)
                        })
                    }
                })
                .collect();

            // Determine the mode:
            // 1. Named captures -> capture_to_idx has at least one Some
            // 2. All unnamed -> capture_to_idx has at least one None beyond index 0
            // 3. No capture groups -> captures_len() == 1
            let has_unnamed = rg.captures_len() > 1 && !has_named_group;

            if has_named_group {
                log::debug!("Named regex: {rg} with {} groups", capture_to_idx.len());

                // Named capture groups
                Arc::new(move |s: &str| {
                    let mut ranges = vec![(0u32, 0u32); col_count];

                    if let Some(caps) = rg.captures(s) {
                        for (group_idx, col_idx_opt) in capture_to_idx.iter().enumerate().skip(1) {
                            if let Some(col_idx) = col_idx_opt {
                                if let Some(m) = caps.get(group_idx) {
                                    ranges[*col_idx] = (m.start() as u32, m.end() as u32);
                                }
                            }
                        }
                    }

                    ranges
                })
            } else if has_unnamed {
                log::debug!(
                    "Unnamed regex: {rg} with {} groups",
                    capture_to_idx.len() - 1
                );

                // All unnamed capture groups -> map in order
                Arc::new(move |s: &str| {
                    let mut ranges = vec![(0u32, 0u32); col_count];
                    if let Some(caps) = rg.captures(s) {
                        for (i, group) in caps.iter().skip(1).enumerate().take(col_count) {
                            if let Some(m) = group {
                                ranges[i] = (m.start() as u32, m.end() as u32);
                            }
                        }
                    }

                    ranges
                })
            } else {
                log::debug!("Delimiter regex: {rg}");

                // No capture groups -> normal delimiter split
                Arc::new(move |s: &str| {
                    let mut ranges = Vec::with_capacity(col_count);
                    let mut last_end = 0;

                    for m in rg.find_iter(s).take(col_count - 1) {
                        ranges.push((last_end as u32, m.start() as u32));
                        last_end = m.end();
                    }

                    ranges.push((last_end as u32, s.len() as u32));
                    ranges
                })
            }
        }
        // allows nontrivial overlaps but rarely useful
        Split::Regexes(ref rgs) => {
            let rgs: Vec<Regex> = rgs.clone();
            Arc::new(move |s: &str| {
                let mut ranges = Vec::with_capacity(col_count);
                let mut last_end = 0;

                for re in rgs.iter().take(col_count) {
                    if last_end <= s.len() {
                        if let Some(m) = re.find(&s[last_end..]) {
                            let start = last_end + m.start();
                            let end = last_end + m.end();
                            ranges.push((start as u32, end as u32));
                            last_end = end;
                            continue;
                        }
                    }
                    ranges.push((0, 0));
                }
                ranges
            })
        }
        Split::None => Arc::new(move |s: &str| vec![(0u32, s.len() as u32)]),
    };

    // Build raw preprocessor (returns string representation)
    let raw_preprocessor: Arc<dyn Fn(&String) -> Option<ConfigPreprocessedData> + Send + Sync> = {
        let split_fn = split_fn.clone();
        let split_clone = split.clone();
        Arc::new(move |item: &String| {
            let s = if trim {
                item.trim().to_string()
            } else {
                item.clone()
            };

            let (plain, ranges) = if ansi {
                let plain = s.as_bytes().into_text().ok()?.to_string();
                let ranges = split_fn(&plain);
                (plain, ranges)
            } else {
                let ranges = split_fn(&s);
                (s, ranges)
            };

            if let Some(c) = require_column {
                let is_no_match = match &split_clone {
                    Split::Delimiter(_) | Split::Regexes(_) => ranges
                        .get(c)
                        .is_none_or(|&(start, end)| start == 0 && end == 0),

                    _ => plain.is_empty(),
                };

                if is_no_match {
                    return None;
                }
            }

            Some((Err(plain), ranges))
        })
    };

    // Build text preprocessor (returns parsed text if ANSI enabled)
    let text_preprocessor: Arc<dyn Fn(&String) -> ConfigPreprocessedData + Send + Sync> = {
        Arc::new(move |item: &String| {
            let s = if trim {
                item.trim().to_string()
            } else {
                item.clone()
            };

            if ansi {
                match s.as_bytes().into_text() {
                    Ok(mut text) => {
                        text::scrub_text_styles(&mut text);
                        let plain = text.to_string();
                        let ranges = split_fn(&plain);
                        (Ok(text), ranges)
                    }
                    Err(_) => {
                        let ranges = split_fn(&s);
                        (Err(s), ranges)
                    }
                }
            } else {
                let ranges = split_fn(&s);
                (Err(s), ranges)
            }
        })
    };

    // Build columns
    let columns: Vec<Column<String, ConfigPreprocessedData>> = column_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            Column::new(
                name.clone(),
                move |_item: &String, d: &ConfigPreprocessedData| {
                    let (text_result, ranges) = d;
                    let range = ranges.get(i).copied().unwrap_or((0, 0));

                    match text_result {
                        Ok(text) => {
                            let mut t =
                                text::slice_ratatui_text(text, range.0 as usize..range.1 as usize);
                            if sanitize {
                                text::apply_to_lines(&mut t, text::sanitize_line);
                            };
                            t
                        }
                        Err(s) => {
                            let s = &s[range.0 as usize..range.1 as usize];
                            if sanitize {
                                Text::from(sanitize_string(s))
                            } else {
                                Text::from(s)
                            }
                        }
                    }
                },
            )
            .with_raw(move |_item: &String, d: &ConfigPreprocessedData| {
                let (_text_result, ranges) = d;
                let range = ranges.get(i).copied().unwrap_or((0, 0));

                match _text_result {
                    Err(s) => Cow::Borrowed(&s[range.0 as usize..range.1 as usize]),
                    Ok(text) => {
                        let s = text.to_string();
                        Cow::Owned(s[range.0 as usize..range.1 as usize].to_string())
                    }
                }
            })
        })
        .collect();

    (columns, raw_preprocessor, text_preprocessor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Split;

    #[test]
    fn test_sanitize_preprocessors() {
        let options = PreprocessConfig {
            ansi: true,
            trim: false,
            require_column: None,
            sanitize: true,
        };
        let (_columns, raw_preprocessor, text_preprocessor) =
            build_columns(options, Split::None, vec![Arc::from("col")]);

        // Input with a tab character, carriage return, and normal characters
        let input = "hello\tworld\r".to_string();

        // 1. Check raw_preprocessor (should NOT sanitize, but ansi: true strips \r)
        let raw_res = raw_preprocessor(&input).unwrap();
        match &raw_res.0 {
            Err(s) => assert_eq!(s, "hello\tworld"),
            _ => panic!("Expected Err(String) containing the raw string"),
        }

        // Check raw_preprocessor without ansi (should be completely untouched)
        let options_no_ansi_raw = PreprocessConfig {
            ansi: false,
            trim: false,
            require_column: None,
            sanitize: true,
        };
        let (_, raw_preprocessor_no_ansi, _) =
            build_columns(options_no_ansi_raw, Split::None, vec![Arc::from("col")]);
        let raw_res_no_ansi = raw_preprocessor_no_ansi(&input).unwrap();
        match &raw_res_no_ansi.0 {
            Err(s) => assert_eq!(s, &input),
            _ => panic!("Expected Err(String)"),
        }

        // 2. Check text_preprocessor (should sanitize in cell format, but preprocessor returns parsed tui text)
        let text_res = text_preprocessor(&input);
        match &text_res.0 {
            Ok(text) => {
                let plain = text.to_string();
                // Carriage return was already stripped by into_text(), tab is preserved
                assert_eq!(plain, "hello\tworld");
            }
            Err(s) => {
                panic!("Expected Ok(Text), got Err({})", s);
            }
        }

        // 3. Test non-ANSI input sanitization
        let options_no_ansi = PreprocessConfig {
            ansi: false,
            trim: false,
            require_column: None,
            sanitize: true,
        };
        let (_, _, text_preprocessor_no_ansi) =
            build_columns(options_no_ansi, Split::None, vec![Arc::from("col")]);
        let text_res_no_ansi = text_preprocessor_no_ansi(&input);
        match &text_res_no_ansi.0 {
            Err(s) => {
                assert_eq!(s, &input);
            }
            _ => panic!("Expected Err(String) for non-ansi path"),
        }
    }
}
