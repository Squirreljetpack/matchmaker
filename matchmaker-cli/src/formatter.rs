use cli_boilerplate_automation::bath::shell_quote_impl;
use cli_boilerplate_automation::unwrap;
use matchmaker::nucleo::Indexed;
use matchmaker::render::MMState;
use matchmaker::{ConfigMMInnerItem, ConfigMMItem};
use std::borrow::Cow;

type ConfigMMState<'a, 'b> = MMState<'a, 'b, ConfigMMItem, ConfigMMInnerItem>;

/// Process_key accepts a ConfigMMInnerItem and uses it in the non-multi branch instead of getting the item from current_raw.
/// Note: Although it accepts Option<..>, it can be considered as accepting a definite ConfigMMInnerItem. The second case with none is unreachable.
/// If repeat is Some(f), and the template contains a non-multi replacement, we use state.map_selected_to_vec. For each selected, use that as the get_current() override. Return String::new().
/// Otherwise, if repeat is None or if the template only consists of non-multi replacement, return a single string, pass the current to process_key. (If state.get_current() is None, return String::new(), which signals no action)
pub fn format_cli(
    state: &ConfigMMState<'_, '_>,
    template: &str,
    repeat: Option<&dyn Fn(String)>,
) -> String {
    if let Some(f) = repeat {
        if any_non_multi(template) {
            state.map_selected_to_vec(|item| {
                let s = format_cli_inner(state, template, Some(item));
                if !s.is_empty() {
                    f(s);
                }
            });
        } else {
            let s = format_cli_inner(state, template, None);
            if !s.is_empty() {
                f(s);
            }
        }
        return String::new();
    }

    if state.current_raw().is_none() && any_non_multi(template) {
        return String::new();
    }

    format_cli_inner(state, template, None)
}

fn any_non_multi(template: &str) -> bool {
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut key = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == '}' {
                    chars.next();
                    let mut k = key.as_str();
                    if k.starts_with('=') {
                        k = &k[1..];
                    }
                    if !k.starts_with('+') && !k.starts_with('-') && !k.contains("..") {
                        return true;
                    }
                    break;
                }
                key.push(chars.next().unwrap());
            }
        }
    }
    false
}

fn format_cli_inner(
    state: &ConfigMMState<'_, '_>,
    template: &str,
    item_override: Option<&ConfigMMInnerItem>,
) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            let mut key = String::new();
            let mut found_end = false;
            while let Some(&nc) = chars.peek() {
                if nc == '}' {
                    chars.next();
                    found_end = true;
                    break;
                }
                key.push(chars.next().unwrap());
            }

            if found_end {
                result.push_str(&process_key(&key, state, item_override));
                continue;
            } else {
                result.push('{');
                result.push_str(&key);
                continue;
            }
        }
        result.push(c);
    }
    result
}

fn process_key(
    key: &str,
    state: &ConfigMMState<'_, '_>,
    item_override: Option<&ConfigMMInnerItem>,
) -> String {
    log::info!("{key}");
    let mut key = key;
    let mut quote = true;
    let mut multi = false;

    if key.starts_with('=') {
        quote = false;
        key = &key[1..];
    } else if key.starts_with('+') {
        multi = true;
        key = &key[1..];
    } else if key.starts_with('-') {
        multi = true;
        quote = false;
        key = &key[1..];
    }

    // Handle ranges
    if key.contains("..") {
        return handle_range(key, state, quote, multi, item_override);
    }

    if multi {
        state
            .map_selected_to_vec(|item| {
                let val = get_val(key, item, state);
                if quote {
                    shell_quote_impl(&val)
                } else {
                    val.into_owned()
                }
            })
            .join(" ")
    } else {
        if let Some(item) = item_override {
            let val = get_val(key, item, state);
            if quote {
                shell_quote_impl(&val)
            } else {
                val.into_owned()
            }
        } else if let Some(item) = state.current_raw() {
            let val = get_val(key, &item.inner, state);
            if quote {
                shell_quote_impl(&val)
            } else {
                val.into_owned()
            }
        } else {
            String::new()
        }
    }
}

fn get_val<'a>(
    key: &str,
    item: &'a ConfigMMInnerItem,
    state: &ConfigMMState<'_, '_>,
) -> Cow<'a, str> {
    if key == "!" {
        // current column
        let col_idx = if key.is_empty() {
            let cursor_byte = state
                .picker_ui
                .input
                .byte_index(state.picker_ui.input.cursor() as usize);
            Some(
                state
                    .picker_ui
                    .worker
                    .query
                    .active_column_index(cursor_byte),
            )
        } else {
            key.parse::<usize>().ok().or_else(|| {
                state
                    .picker_ui
                    .worker
                    .columns
                    .iter()
                    .position(|c| c.name.as_ref() == key)
            })
        };

        if let Some(idx) = col_idx {
            if let Some(col) = state.picker_ui.worker.columns.get(idx) {
                let indexed = Indexed {
                    index: 0,
                    inner: item.clone(),
                };
                return col.format_text(&indexed).to_string().into();
            }
        }
        Cow::Borrowed("")
    } else {
        if key.is_empty() {
            item.to_cow()
        } else {
            // Try to use key as column index or name
            let col_idx = key.parse::<usize>().ok().or_else(|| {
                state
                    .picker_ui
                    .worker
                    .columns
                    .iter()
                    .position(|c| c.name.as_ref() == key)
            });

            if let Some(idx) = col_idx {
                if let Some(col) = state.picker_ui.worker.columns.get(idx) {
                    let indexed = Indexed {
                        index: 0,
                        inner: item.clone(),
                    };
                    return col.format_text(&indexed).to_string().into();
                }
            }
            Cow::Borrowed("")
        }
    }
}

fn handle_range<'a, 'b>(
    key: &str,
    state: &ConfigMMState<'_, '_>,
    quote: bool,
    multi: bool,
    item_override: Option<&ConfigMMInnerItem>,
) -> String {
    let parts: Vec<&str> = key.split("..").collect();
    let start_key = parts.get(0).copied().unwrap_or("");
    let end_key = parts.get(1).copied().unwrap_or("");

    let start_idx = if start_key.is_empty() {
        0
    } else {
        let ret = state
            .picker_ui
            .worker
            .columns
            .iter()
            .position(|c| c.name.as_ref() == start_key);
        unwrap!(ret)
    };

    let end_idx = if end_key.is_empty() {
        state.picker_ui.worker.columns.len()
    } else {
        let ret = state
            .picker_ui
            .worker
            .columns
            .iter()
            .position(|c| c.name.as_ref() == end_key);
        unwrap!(ret)
    };

    if start_idx >= state.picker_ui.worker.columns.len()
        || (end_idx == 0 && !end_key.is_empty())
        || start_idx > end_idx
    {
        log::error!(
            "Multi-format indexing error: start: {start_idx}, end: {end_idx}, columns: {}",
            state.picker_ui.worker.columns.len()
        );
        return String::new();
    }

    let columns_to_join: Vec<usize> = (start_idx..end_idx)
        .filter(|&i| {
            i >= state.picker_ui.results.hidden_columns.len()
                || !state.picker_ui.results.hidden_columns[i]
        })
        .collect();

    if multi {
        state
            .map_selected_to_vec(|item| {
                let mut row_res = Vec::new();
                let indexed = Indexed {
                    index: 0,
                    inner: item.clone(),
                };
                for &col_idx in &columns_to_join {
                    let col = &state.picker_ui.worker.columns[col_idx];
                    let val = col.format_text(&indexed).to_string();
                    row_res.push(val);
                }
                let joined = row_res.join(" ");
                if quote {
                    shell_quote_impl(&joined)
                } else {
                    joined
                }
            })
            .join(" ")
    } else {
        if let Some(item) = item_override {
            let mut row_res = Vec::new();
            let indexed = Indexed {
                index: 0,
                inner: item.clone(),
            };
            for &col_idx in &columns_to_join {
                let col = &state.picker_ui.worker.columns[col_idx];
                let val = col.format_text(&indexed).to_string();
                row_res.push(val);
            }
            let joined = row_res.join(" ");
            if quote {
                shell_quote_impl(&joined)
            } else {
                joined
            }
        } else if let Some(item) = state.current_raw() {
            let mut row_res = Vec::new();
            for &col_idx in &columns_to_join {
                let col = &state.picker_ui.worker.columns[col_idx];
                let val = col.format_text(item).to_string();
                row_res.push(val);
            }
            let joined = row_res.join(" ");
            if quote {
                shell_quote_impl(&joined)
            } else {
                joined
            }
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matchmaker::config::{ExitConfig, RenderConfig, TerminalConfig, WorkerConfig};
    use matchmaker::nucleo::injector::{Injector, PreprocessOptions};
    use matchmaker::nucleo::nucleo::{Config as NucleoConfig, Matcher};
    use matchmaker::render::State;
    use matchmaker::ui::UI;
    use tokio::sync::mpsc;

    fn setup_test_mm() -> (matchmaker::ConfigMatchmaker, matchmaker::ConfigInjector) {
        let render_config = RenderConfig::default();
        let tui_config = TerminalConfig::default();
        let mut worker_config = WorkerConfig::default();
        worker_config.columns.names = vec![
            matchmaker::config::ColumnSetting {
                name: "col1".to_string(),
                filter: true,
                hidden: false,
            },
            matchmaker::config::ColumnSetting {
                name: "col2".to_string(),
                filter: true,
                hidden: false,
            },
            matchmaker::config::ColumnSetting {
                name: "col3".to_string(),
                filter: true,
                hidden: false,
            },
        ];
        worker_config.columns.split =
            matchmaker::config::Split::Delimiter(regex::Regex::new(",").unwrap());
        let exit_config = ExitConfig::default();
        let preprocess_config: PreprocessOptions = (false, false);

        let (mm, injector, _misc) = matchmaker::ConfigMatchmaker::new_from_config(
            render_config,
            tui_config,
            worker_config,
            exit_config,
            preprocess_config,
        );
        (mm, injector)
    }

    #[tokio::test]
    async fn test_format_cli_basic() {
        let (mut mm, injector) = setup_test_mm();
        injector.push("a,b,c".to_string()).unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let mut tui = matchmaker::tui::Tui::new(TerminalConfig::default()).unwrap();
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);

        let hidden_columns = vec![false, false, false];
        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            mm.selector,
            None,
            &mut tui,
            hidden_columns,
        );

        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        {
            let mut mm_state = state_obj.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &event_tx,
            );

            let result = format_cli(&mut mm_state, "echo {col1} {=col2} {col3}", None);
            assert_eq!(result, "echo 'a' b 'c'");
        }
    }

    #[tokio::test]
    async fn test_format_cli_ranges() {
        let (mut mm, injector) = setup_test_mm();
        injector.push("a,b,c".to_string()).unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let mut tui = matchmaker::tui::Tui::new(TerminalConfig::default()).unwrap();
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);

        let hidden_columns = vec![false, false, false];
        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            mm.selector,
            None,
            &mut tui,
            hidden_columns,
        );

        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        {
            let mut mm_state = state_obj.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &event_tx,
            );

            let result = format_cli(&mut mm_state, "echo {..} {col2..} {..col2}", None);
            // ..col2 is exclusive
            assert_eq!(result, "echo 'a b c' 'b c' 'a'");

            let result = format_cli(&mut mm_state, "echo {=col2..} {-..col2}", None);
            // ..col2 is exclusive
            assert_eq!(result, "echo b c a");
        }
    }

    #[tokio::test]
    async fn test_format_cli_selections() {
        let (mut mm, injector) = setup_test_mm();
        injector.push("a,b,c".to_string()).unwrap();
        injector.push("1,2,3".to_string()).unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let mut tui = matchmaker::tui::Tui::new(TerminalConfig::default()).unwrap();
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);

        let hidden_columns = vec![false, false, false];
        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            mm.selector,
            None,
            &mut tui,
            hidden_columns,
        );

        // Select both items
        let item1 = picker_ui.worker.get_nth(0).unwrap().clone();
        let item2 = picker_ui.worker.get_nth(1).unwrap().clone();
        picker_ui.selector.sel(&item1);
        picker_ui.selector.sel(&item2);

        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        {
            let mut mm_state = state_obj.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &event_tx,
            );

            // Set query to select col2
            mm_state.picker_ui.input.set(Some("%col2 ".to_string()), 6);
            mm_state.picker_ui.update();

            let result = format_cli(&mut mm_state, "echo {+} {-col1} {-!} {+!}", None);
            // {+} -> 'a,b,c' '1,2,3'
            // {-col1} -> a 1
            // {-!} -> b 2 (active col is col2 because of %col2 )
            // {+!} -> 'b' '2'
            assert_eq!(result, "echo 'a,b,c' '1,2,3' a 1 b 2 'b' '2'");
        }
    }
}
