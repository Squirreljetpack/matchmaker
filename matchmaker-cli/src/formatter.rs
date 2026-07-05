use cba::broc::shell_quote;
use cba::unwrap;
use matchmaker::config_mm::ConfigPreprocessedData;
use matchmaker::render::MMState;
use std::borrow::Cow;

// support {1} -> first column
const COLUMN_INDICES: bool = true;

type ConfigMMState<'a, 'b> = MMState<'a, 'b, String, ConfigPreprocessedData>;

fn is_valid_key(s: &str) -> bool {
    let body = s.strip_prefix(&['=', '-', '_', '+'][..]).unwrap_or(s);
    if body.is_empty() || body == "!" || body == "#" {
        return true;
    }

    if let Some(num) = body.strip_prefix('$')
        && num.chars().all(|c| c.is_ascii_digit())
        && !num.is_empty()
    {
        return true;
    }

    body.chars().all(|c| c.is_alphanumeric())
}

fn is_valid_content(s: &str) -> bool {
    // Check if it's a key..key range
    if let Some(idx) = s.find("..") {
        is_valid_key(&s[..idx]) && is_valid_key(&s[idx + 2..])
    } else {
        // Or just a single key
        is_valid_key(s)
    }
}

/// Process_key accepts a String and uses it in the non-multi branch instead of getting the item from current_raw.
/// Note: Although it accepts Option<..>, it can be considered as accepting a definite String. The second case with none is unreachable.
/// If repeat is Some(f), and the template contains a non-multi replacement, we use state.map_selected_to_vec. For each selected, use that as the get_current() override. Return String::new().
/// Otherwise, if repeat is None or if the template only consists of non-multi replacement, return a single string, pass the current to process_key. (If state.get_current() is None, return String::new(), which signals no action)
pub fn format_cli(
    state: &ConfigMMState<'_, '_>,
    template: &str,
    repeat: Option<&dyn Fn(String)>,
) -> String {
    if template.is_empty() {
        return String::new();
    }
    if let Some(f) = repeat {
        if any_need_current(template) {
            state.map_selected_to_vec(|i, item| {
                let s = format_cli_inner(state, template, Some((i, item)));
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

    if state.current_raw().is_none() && any_need_current(template) {
        return String::new();
    }

    format_cli_inner(state, template, None)
}

fn format_cli_inner(
    state: &ConfigMMState<'_, '_>,
    template: &str,
    item_override: Option<(u32, &String)>,
) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();

    'outer: while let Some((_, c)) = chars.next() {
        if c == '\\' {
            if let Some(&(_, next)) = chars.peek()
                && next == '{'
            {
                chars.next();
                result.push('{');
                continue;
            }
            result.push('\\');
            continue;
        }

        if c == '{' {
            // no more chars
            let Some(&(start, _)) = chars.peek() else {
                result.push('{');
                break;
            };

            while let Some(&(j, nc)) = chars.peek() {
                if nc == '{' {
                    // Nested '{' found: push what we have so far as literal
                    // and let the outer loop consume the new '{'
                    result.push('{');
                    result.push_str(&template[start..j]);
                    continue 'outer;
                }

                chars.next();

                if nc == '}' {
                    let key = &template[start..j];

                    if is_valid_content(key)
                        && let Some(s) = process_key(key, state, item_override)
                    {
                        result.push_str(&s);
                    } else {
                        // Invalid key
                        result.push('{');
                        result.push_str(key);
                        result.push('}');
                    }
                    continue 'outer;
                }
            }

            // No closing brace
            result.push('{');
            result.push_str(&template[start..]);
            break;
        }

        result.push(c);
    }

    result
}

fn any_need_current(template: &str) -> bool {
    let mut chars = template.char_indices().peekable();

    'outer: while let Some((_, c)) = chars.next() {
        if c == '\\' {
            if let Some(&(_, next)) = chars.peek()
                && next == '{'
            {
                chars.next();
            }
            continue;
        }

        if c == '{' {
            let Some(&(start, _)) = chars.peek() else {
                break;
            };

            while let Some(&(j, nc)) = chars.peek() {
                if nc == '{' {
                    continue 'outer;
                }

                chars.next();

                if nc == '}' {
                    let key = &template[start..j];

                    // Check valid content and slice match for prefixes
                    if is_valid_content(key) && !key.starts_with(['+', '-', '$']) {
                        return true;
                    }
                    continue 'outer;
                }
            }
        }
    }

    false
}

fn process_key(
    input: &str,
    state: &ConfigMMState<'_, '_>,
    item_override: Option<(u32, &String)>,
) -> Option<String> {
    let mut key = input;
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

    if let Some(num_str) = key.strip_prefix('$')
        && let Ok(idx) = num_str.parse::<usize>()
    {
        let args = crate::start::COMMAND_ARGS.lock().unwrap();
        // return all args joined
        return if idx == 0 {
            let joined = args
                .iter()
                .map(|arg| {
                    if quote {
                        shell_quote(&arg)
                    } else {
                        arg.to_str().map(str::to_string)
                    }
                })
                .collect::<Option<Vec<_>>>()?
                .join(" ");
            Some(joined)
        } else if let Some(arg) = args.get(idx - 1) {
            if quote {
                shell_quote(&arg)
            } else {
                arg.to_str().map(str::to_string)
            }
        } else {
            Some(String::new())
        };
    }

    // Handle ranges
    if key.contains("..") {
        return handle_range(key, state, quote, multi, item_override.map(|x| x.1));
    }

    if multi {
        Some(
            state
                .map_selected_to_vec(|i, item| {
                    let val = get_val(key, (i, item), state).unwrap_or(Cow::Borrowed(""));
                    if quote {
                        shell_quote(val.as_ref()).unwrap()
                    } else {
                        val.to_string()
                    }
                })
                .join(" "),
        )
    } else {
        let item = unwrap!(item_override.or_else(|| state.picker_ui.current_indexed()));

        let val = get_val(key, item, state)?;
        if quote {
            shell_quote(val.as_ref())
        } else {
            Some(val.into_owned())
        }
    }
}

fn get_val<'a>(
    key: &str,
    (index, item): (u32, &'a String),
    state: &ConfigMMState<'_, '_>,
) -> Option<Cow<'a, str>> {
    if key == "!" {
        // current column
        let idx = state.picker_ui.active_column_index();

        if let Some(col) = state.picker_ui.worker.columns.get(idx) {
            let d = (state.picker_ui.worker.raw_preprocessor)(item)?;
            return Some(col.raw(item, &d).to_string().into());
        }
        None
    } else {
        if key.is_empty() {
            Some(Cow::Borrowed(item.as_str()))
        } else if key == "#" {
            Some(index.to_string().into())
        } else {
            // Try to use key as column index or name
            let col_idx = state
                .picker_ui
                .worker
                .columns
                .iter()
                .position(|c| c.name.as_ref() == key);

            let idx = if let Some(i) = col_idx {
                Some(i)
            } else if COLUMN_INDICES {
                key.parse::<usize>().ok().map(|x| x.saturating_sub(1))
            } else {
                None
            };

            if let Some(idx) = idx
                && let Some(col) = state.picker_ui.worker.columns.get(idx)
            {
                let d = (state.picker_ui.worker.raw_preprocessor)(item)?;
                return Some(col.raw(item, &d).to_string().into());
            }

            None
        }
    }
}

fn handle_range<'a, 'b>(
    key: &str,
    state: &ConfigMMState<'_, '_>,
    quote: bool,
    multi: bool,
    item_override: Option<&String>,
) -> Option<String> {
    let parts: Vec<&str> = key.split("..").collect();
    let start_key = parts.first().copied().unwrap_or("");
    let end_key = parts.get(1).copied().unwrap_or("");

    let start_idx = if start_key.is_empty() {
        0
    } else {
        state
            .picker_ui
            .worker
            .columns
            .iter()
            .position(|c| c.name.as_ref() == start_key)?
    };

    let end_idx = if end_key.is_empty() {
        state.picker_ui.worker.columns.len()
    } else {
        state
            .picker_ui
            .worker
            .columns
            .iter()
            .position(|c| c.name.as_ref() == end_key)?
    };

    if start_idx >= state.picker_ui.worker.columns.len()
        || (end_idx == 0 && !end_key.is_empty())
        || start_idx > end_idx
    {
        log::error!(
            "Multi-format indexing error: start: {start_idx}, end: {end_idx}, columns: {}",
            state.picker_ui.worker.columns.len()
        );
        return None;
    }

    let columns_to_join: Vec<usize> = (start_idx..end_idx)
        .filter(|&i| state.picker_ui.results.hidden_cols().contains(i))
        .collect();

    if multi {
        Some(
            state
                .map_selected_to_vec(|_, item| {
                    let mut row_res = Vec::new();
                    let d = match (state.picker_ui.worker.raw_preprocessor)(item) {
                        Some(d) => d,
                        None => return String::new(),
                    };
                    for &col_idx in &columns_to_join {
                        let col = &state.picker_ui.worker.columns[col_idx];
                        let val = col.raw(item, &d).to_string();
                        row_res.push(val);
                    }
                    let joined = row_res.join(" ");
                    if quote {
                        shell_quote(&joined).unwrap()
                    } else {
                        joined
                    }
                })
                .join(" "),
        )
    } else {
        if let Some(item) = item_override {
            let mut row_res = Vec::new();
            let d = (state.picker_ui.worker.raw_preprocessor)(item)?;
            for &col_idx in &columns_to_join {
                let col = &state.picker_ui.worker.columns[col_idx];
                let val = col.raw(item, &d).to_string();
                row_res.push(val);
            }
            let joined = row_res.join(" ");
            if quote {
                Some(shell_quote(&joined).unwrap())
            } else {
                Some(joined)
            }
        } else if let Some(item) = state.current_raw() {
            let mut row_res = Vec::new();
            let d = (state.picker_ui.worker.raw_preprocessor)(item)?;
            for &col_idx in &columns_to_join {
                let col = &state.picker_ui.worker.columns[col_idx];
                let val = col.raw(item, &d).to_string();
                row_res.push(val);
            }
            let joined = row_res.join(" ");
            if quote {
                Some(shell_quote(&joined).unwrap())
            } else {
                Some(joined)
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matchmaker::Selector;
    use matchmaker::config::{ColumnsConfig, TerminalConfig};
    use matchmaker::config_mm::{ConfigInjector, ConfigMatchmaker};
    use matchmaker::nucleo::injector::Injector;
    use matchmaker::nucleo::nucleo::{Config as NucleoConfig, Matcher};
    use matchmaker::render::State;
    use matchmaker::ui::UI;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn setup_test_mm() -> (
        ConfigMatchmaker,
        ConfigInjector,
        Result<
            std::sync::MutexGuard<'static, ()>,
            std::sync::PoisonError<std::sync::MutexGuard<'static, ()>>,
        >,
    ) {
        let guard = TEST_MUTEX.lock();
        let mut columns_config = ColumnsConfig::default();
        columns_config.names = vec![
            matchmaker::config::ColumnSetting {
                name: "col1".to_string().into(),
                ignore: true,
                hidden: false,
                options: Default::default(),
            },
            matchmaker::config::ColumnSetting {
                name: "col2".to_string().into(),
                ignore: true,
                hidden: false,
                options: Default::default(),
            },
            matchmaker::config::ColumnSetting {
                name: "col3".to_string().into(),
                ignore: true,
                hidden: false,
                options: Default::default(),
            },
        ];
        columns_config.split =
            matchmaker::config::Split::Delimiter(regex::Regex::new(",").unwrap());

        let (mm, injector, _misc) = ConfigMatchmaker::new_from_config(
            Default::default(),
            Default::default(),
            Default::default(),
            columns_config,
            Default::default(),
            Default::default(),
        );
        (mm, injector, guard)
    }

    #[tokio::test]
    async fn test_format_cli_basic() {
        let (mut mm, injector, _guard) = setup_test_mm();
        injector.push("a,b,c".to_string()).unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let Ok(mut tui) = matchmaker::tui::Tui::new(TerminalConfig::default()) else {
            return;
        };
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);

        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            Selector::new(),
            None,
            &mut tui,
            vec![],
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

            let result = format_cli(&mut mm_state, "echo {} {=}", None);
            assert_eq!(result, "echo 'a,b,c' a,b,c");

            let result = format_cli(&mut mm_state, "echo {{col1}} {{=col2}}", None);
            assert_eq!(result, "echo {'a'} {b}");

            let result = format_cli(&mut mm_state, "echo {col1 } {col1:val}", None);
            assert_eq!(result, "echo {col1 } {col1:val}");

            let result = format_cli(&mut mm_state, "echo { {} }", None);
            assert_eq!(result, "echo { 'a,b,c' }");
        }
    }

    #[tokio::test]
    async fn test_format_cli_ranges() {
        let (mut mm, injector, _guard) = setup_test_mm();
        injector.push("a,b,c".to_string()).unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let Ok(mut tui) = matchmaker::tui::Tui::new(TerminalConfig::default()) else {
            return;
        };
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);

        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            Selector::new(),
            None,
            &mut tui,
            vec![],
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
        let (mut mm, injector, _guard) = setup_test_mm();
        injector.push("a,b,c".to_string()).unwrap();
        injector.push("1,2,3".to_string()).unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let Ok(mut tui) = matchmaker::tui::Tui::new(TerminalConfig::default()) else {
            return;
        };
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);

        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            Selector::new(),
            None,
            &mut tui,
            vec![],
        );

        // Select both items
        let (idx1, _) = picker_ui.worker.get_nth_indexed(0).unwrap();
        let (idx2, _) = picker_ui.worker.get_nth_indexed(1).unwrap();
        picker_ui.selector.insert(idx1);
        picker_ui.selector.insert(idx2);

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
            mm_state.picker_ui.query.set(Some("%col2 ".to_string()), 6);
            mm_state.picker_ui.update();

            let result = format_cli(&mut mm_state, "echo {+} {-col1} {-!} {+!}", None);
            dbg!(picker_ui.selector);
            // {+} -> 'a,b,c' '1,2,3'
            // {-col1} -> a 1
            // {-!} -> b 2 (active col is col2 because of %col2 )
            // {+!} -> 'b' '2'
            assert_eq!(result, "echo 'a,b,c' '1,2,3' a 1 b 2 'b' '2'");
        }
    }

    #[tokio::test]
    async fn test_format_cli_invalid_key() {
        let (mut mm, injector, _guard) = setup_test_mm();
        injector.push("a,b,c".to_string()).unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let Ok(mut tui) = matchmaker::tui::Tui::new(TerminalConfig::default()) else {
            return;
        };
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);

        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            Selector::new(),
            None,
            &mut tui,
            vec![],
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

            let result = format_cli(&mut mm_state, "echo {missing} {=also_invalid}", None);
            assert_eq!(result, "echo {missing} {=also_invalid}");
        }
    }

    #[tokio::test]
    async fn test_format_cli_command_args() {
        {
            let mut args = crate::start::COMMAND_ARGS.lock().unwrap();
            args.clear();
            args.push("arg1".into());
            args.push("arg with space".into());
        }

        let (mut mm, injector, _guard) = setup_test_mm();
        injector.push("a,b,c".to_string()).unwrap();
        mm.worker.nucleo.tick(10);

        let mut state_obj = State::new();
        let Ok(mut tui) = matchmaker::tui::Tui::new(TerminalConfig::default()) else {
            return;
        };
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);

        let (mut ui, mut picker_ui, mut footer_ui, mut preview_ui) = UI::new(
            mm.render_config,
            &mut matcher,
            mm.worker,
            Selector::new(),
            None,
            &mut tui,
            vec![],
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

            let result = format_cli(&mut mm_state, "echo {$0} {=$0}", None);
            assert_eq!(result, "echo 'arg1' 'arg with space' arg1 arg with space");

            let result = format_cli(&mut mm_state, "echo {$1} {=$2} {$3}", None);
            assert_eq!(result, "echo 'arg1' arg with space ");
        }
    }

    // #[tokio::test]
    // async fn test_skip_empty() {
    //     use matchmaker::config_mm::ConfigMatchmaker;
    //     let mut columns_config = matchmaker::config::ColumnsConfig::default();
    //     columns_config.names = vec![
    //         matchmaker::config::ColumnSetting {
    //             name: "col1".to_string().into(),
    //             ignore: false,
    //             hidden: false,
    //             options: Default::default(),
    //         },
    //         matchmaker::config::ColumnSetting {
    //             name: "col2".to_string().into(),
    //             ignore: false,
    //             hidden: false,
    //             options: Default::default(),
    //         },
    //     ];
    //     columns_config.split =
    //         matchmaker::config::Split::Delimiter(regex::Regex::new(",").unwrap());

    //     // (parse_ansi, trim, skip_empty = true)
    //     let preprocess = (false, true, true);

    //     let (mut mm, injector, _misc) = ConfigMatchmaker::new_from_config(
    //         Default::default(),
    //         Default::default(),
    //         Default::default(),
    //         columns_config,
    //         Default::default(),
    //         preprocess,
    //     );

    //     injector.push("a,b".to_string()).unwrap();
    //     injector.push("".to_string()).unwrap(); // should be skipped
    //     injector.push("  ".to_string()).unwrap(); // should be trimmed to empty and skipped
    //     injector.push("c,d".to_string()).unwrap();

    //     mm.worker.nucleo.tick(10);
    //     let count = mm.worker.counts().1; // total item count
    //     assert_eq!(count, 2);
    // }

    // #[tokio::test]
    // async fn test_skip_no_match() {
    //     use matchmaker::config_mm::ConfigMatchmaker;
    //     let mut columns_config = matchmaker::config::ColumnsConfig::default();
    //     columns_config.names = vec![
    //         matchmaker::config::ColumnSetting {
    //             name: "col1".to_string().into(),
    //             ignore: false,
    //             hidden: false,
    //             options: Default::default(),
    //         },
    //         matchmaker::config::ColumnSetting {
    //             name: "col2".to_string().into(),
    //             ignore: false,
    //             hidden: false,
    //             options: Default::default(),
    //         },
    //     ];
    //     // Regex with capture groups
    //     columns_config.split =
    //         matchmaker::config::Split::Delimiter(regex::Regex::new(r"^([a-z]+)-([a-z]+)$").unwrap());

    //     // (parse_ansi, trim, skip_empty = true)
    //     let preprocess = (false, true, true);

    //     let (mut mm, injector, _misc) = ConfigMatchmaker::new_from_config(
    //         Default::default(),
    //         Default::default(),
    //         Default::default(),
    //         columns_config,
    //         Default::default(),
    //         preprocess,
    //     );

    //     injector.push("abc-def".to_string()).unwrap(); // matches -> kept
    //     injector.push("abc".to_string()).unwrap(); // no match -> skipped
    //     injector.push("abc-".to_string()).unwrap(); // no match -> skipped
    //     injector.push("xyz-uvw".to_string()).unwrap(); // matches -> kept

    //     mm.worker.nucleo.tick(10);
    //     let count = mm.worker.counts().1; // total item count
    //     assert_eq!(count, 2);
    // }
}
