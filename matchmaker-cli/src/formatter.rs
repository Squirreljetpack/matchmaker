use cba::broc::shell_quote;
use cba::unwrap;
use matchmaker::config_mm::ConfigPreprocessedData;
use matchmaker::render::MMState;
use std::borrow::Cow;

// support {1} -> first column
const COLUMN_INDICES: bool = true;

type ConfigMMState<'a> = MMState<'a, String, ConfigPreprocessedData>;

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
    state: &ConfigMMState<'_>,
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
    state: &ConfigMMState<'_>,
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
    state: &ConfigMMState<'_>,
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
                        shell_quote(arg)
                    } else {
                        arg.to_str().map(str::to_string)
                    }
                })
                .collect::<Option<Vec<_>>>()?
                .join(" ");
            Some(joined)
        } else if let Some(arg) = args.get(idx - 1) {
            if quote {
                shell_quote(arg)
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

/// Resolve a template key to a column index.
///
/// Numeric keys are always treated as column indices (never column names):
/// `0` resolves to the primary column, `1` to the first column, etc.
/// Non-numeric keys are looked up by column name.
fn column_index_for_key(key: &str, state: &ConfigMMState<'_>) -> Option<usize> {
    if let Ok(n) = key.parse::<usize>() {
        if COLUMN_INDICES {
            Some(if n == 0 {
                state.picker_ui.worker.query.primary_column_index()
            } else {
                n - 1
            })
        } else {
            None
        }
    } else {
        state
            .picker_ui
            .worker
            .columns
            .iter()
            .position(|c| c.name.as_ref() == key)
    }
}

fn get_val<'a>(
    key: &str,
    (index, item): (u32, &'a String),
    state: &ConfigMMState<'_>,
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
            // Numeric keys are always treated as column indices, never as
            // column names. Non-numeric keys are looked up by name.
            let idx = column_index_for_key(key, state);

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

fn handle_range(
    key: &str,
    state: &ConfigMMState<'_>,
    quote: bool,
    multi: bool,
    item_override: Option<&String>,
) -> Option<String> {
    let parts: Vec<&str> = key.split("..").collect();
    let start_key = parts.first().copied().unwrap_or("");
    let end_key = parts.get(1).copied().unwrap_or("");

    // Same index-first resolution as get_val: numeric keys are treated as
    let start_idx = if start_key.is_empty() {
        0
    } else {
        column_index_for_key(start_key, state)?
    };

    let end_idx = if end_key.is_empty() {
        state.picker_ui.worker.columns.len()
    } else {
        column_index_for_key(end_key, state)?
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
        .filter(|&i| !state.picker_ui.results.hidden_cols().contains(i))
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
pub(crate) mod tests {
    use super::*;
    use matchmaker::config::{ColumnsConfig, PreprocessConfig};
    use matchmaker::config_mm::{ConfigInjector, ConfigMatchmaker};
    use matchmaker::nucleo::injector::Injector;
    use matchmaker::nucleo::new_snapshot;
    use matchmaker::render::State;
    use matchmaker::ui::{DisplayUI, PickerUI, UI};
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    pub(crate) fn setup_test_mm() -> (
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
            columns_config,
            Default::default(),
            Default::default(),
        );
        (mm, injector, guard)
    }

    /// Builds the picker offline (no terminal) for formatting tests.
    pub(crate) fn offline_ui(
        mm: ConfigMatchmaker,
    ) -> (UI, PickerUI<String, ConfigPreprocessedData>) {
        UI::new_offline(mm.render_config, mm.worker)
    }

    /// Pushes items and waits until the worker has indexed `expected` of them.
    /// `tick` alone is racy: the worker thread may not have finished processing
    /// the queue, so we sync on the snapshot like the `--list` implementation.
    pub(crate) fn push_items(
        mm: &mut ConfigMatchmaker,
        injector: ConfigInjector,
        items: &[&str],
        expected: usize,
    ) {
        for item in items {
            injector.push(item.to_string()).unwrap();
        }
        drop(injector);
        let status = loop {
            let (_, s) = new_snapshot(&mut mm.worker.nucleo);
            if s.item_count as usize == expected && !s.running {
                break s;
            }
        };
        assert_eq!(status.item_count as usize, expected);
    }

    #[tokio::test]
    async fn test_format_cli_basic() {
        let (mut mm, injector, _guard) = setup_test_mm();
        push_items(&mut mm, injector, &["a,b,c"], 1);
        let mut state_obj = State::new();

        let (mut ui, mut picker_ui) = offline_ui(mm);
        let mut footer_ui = DisplayUI::default();
        let mut preview_ui = None;

        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        {
            let mm_state = state_obj.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &event_tx,
            );

            let result = format_cli(&mm_state, "echo {col1} {=col2} {col3}", None);
            assert_eq!(result, "echo 'a' b 'c'");

            let result = format_cli(&mm_state, "echo {} {=}", None);
            assert_eq!(result, "echo 'a,b,c' a,b,c");

            let result = format_cli(&mm_state, "echo {{col1}} {{=col2}}", None);
            assert_eq!(result, "echo {'a'} {b}");

            let result = format_cli(&mm_state, "echo {col1 } {col1:val}", None);
            assert_eq!(result, "echo {col1 } {col1:val}");

            let result = format_cli(&mm_state, "echo { {} }", None);
            assert_eq!(result, "echo { 'a,b,c' }");
        }
    }

    #[tokio::test]
    async fn test_format_cli_ranges() {
        let (mut mm, injector, _guard) = setup_test_mm();
        push_items(&mut mm, injector, &["a,b,c"], 1);
        let mut state_obj = State::new();

        let (mut ui, mut picker_ui) = offline_ui(mm);
        let mut footer_ui = DisplayUI::default();
        let mut preview_ui = None;

        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        {
            let mm_state = state_obj.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &event_tx,
            );

            let result = format_cli(&mm_state, "echo {..} {col2..} {..col2}", None);
            // ..col2 is exclusive
            assert_eq!(result, "echo 'a b c' 'b c' 'a'");

            let result = format_cli(&mm_state, "echo {=col2..} {-..col2}", None);
            // ..col2 is exclusive
            assert_eq!(result, "echo b c a");
        }
    }

    #[tokio::test]
    async fn test_format_cli_selections() {
        let (mut mm, injector, _guard) = setup_test_mm();
        push_items(&mut mm, injector, &["a,b,c", "1,2,3"], 2);

        let mut state_obj = State::new();

        let (mut ui, mut picker_ui) = offline_ui(mm);
        let mut footer_ui = DisplayUI::default();
        let mut preview_ui = None;

        // Select both items
        let (idx1, _) = picker_ui.worker.get_nth_indexed(0).unwrap();
        let (idx2, _) = picker_ui.worker.get_nth_indexed(1).unwrap();
        picker_ui.selector.insert(idx1);
        picker_ui.selector.insert(idx2);

        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        {
            let mm_state = state_obj.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &event_tx,
            );

            // Set query to select col2
            mm_state.picker_ui.query.set(Some("%col2 ".to_string()), 6);
            mm_state.picker_ui.update();

            let result = format_cli(&mm_state, "echo {+} {-col1} {-!} {+!}", None);
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
        push_items(&mut mm, injector, &["a,b,c"], 1);
        let mut state_obj = State::new();

        let (mut ui, mut picker_ui) = offline_ui(mm);
        let mut footer_ui = DisplayUI::default();
        let mut preview_ui = None;

        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        {
            let mm_state = state_obj.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &event_tx,
            );

            let result = format_cli(&mm_state, "echo {missing} {=also_invalid}", None);
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
        push_items(&mut mm, injector, &["a,b,c"], 1);
        let mut state_obj = State::new();

        let (mut ui, mut picker_ui) = offline_ui(mm);
        let mut footer_ui = DisplayUI::default();
        let mut preview_ui = None;

        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        {
            let mm_state = state_obj.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &event_tx,
            );

            let result = format_cli(&mm_state, "echo {$0} {=$0}", None);
            assert_eq!(result, "echo 'arg1' 'arg with space' arg1 arg with space");

            let result = format_cli(&mm_state, "echo {$1} {=$2} {$3}", None);
            assert_eq!(result, "echo 'arg1' arg with space ");
        }
    }

    #[tokio::test]
    async fn test_skip_empty() {
        let mut columns_config = ColumnsConfig::default();
        columns_config.names = vec![
            matchmaker::config::ColumnSetting {
                name: "col1".to_string().into(),
                ignore: false,
                hidden: false,
                options: Default::default(),
            },
            matchmaker::config::ColumnSetting {
                name: "col2".to_string().into(),
                ignore: false,
                hidden: false,
                options: Default::default(),
            },
        ];
        columns_config.split =
            matchmaker::config::Split::Delimiter(regex::Regex::new(",").unwrap());

        // ansi: false, trim: true, require the first column to be non-empty
        let preprocess = PreprocessConfig {
            ansi: false,
            trim: true,
            sanitize: false,
            require_column: Some(0),
        };

        let (mut mm, injector, _misc) = ConfigMatchmaker::new_from_config(
            Default::default(),
            Default::default(),
            columns_config,
            Default::default(),
            preprocess,
        );

        push_items(&mut mm, injector, &["a,b", "", "  ", "c,d"], 2);
        let count = mm.worker.counts().1; // total item count
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_skip_no_match() {
        let mut columns_config = ColumnsConfig::default();
        columns_config.names = vec![
            matchmaker::config::ColumnSetting {
                name: "col1".to_string().into(),
                ignore: false,
                hidden: false,
                options: Default::default(),
            },
            matchmaker::config::ColumnSetting {
                name: "col2".to_string().into(),
                ignore: false,
                hidden: false,
                options: Default::default(),
            },
        ];
        // Regex with capture groups
        columns_config.split = matchmaker::config::Split::Delimiter(
            regex::Regex::new(r"^([a-z]+)-([a-z]+)$").unwrap(),
        );

        // ansi: false, trim: true, require the first column to be non-empty
        let preprocess = PreprocessConfig {
            ansi: false,
            trim: true,
            sanitize: false,
            require_column: Some(0),
        };

        let (mut mm, injector, _misc) = ConfigMatchmaker::new_from_config(
            Default::default(),
            Default::default(),
            columns_config,
            Default::default(),
            preprocess,
        );

        push_items(&mut mm, injector, &["abc-def", "abc", "abc-", "xyz-uvw"], 2);
        let count = mm.worker.counts().1; // total item count
        assert_eq!(count, 2);
    }
}
