use cli_boilerplate_automation::bring::consume_escaped;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Substitute characters present as keys in the map, unless they are escaped.
pub fn substitute_escaped<U: AsRef<str>>(input: &str, map: &[(char, U)]) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&k) => {
                    if let Some((_, replacement)) = map.iter().find(|(key, _)| *key == k) {
                        out.push_str(replacement.as_ref());
                        chars.next();
                    } else {
                        out.push('\\');
                        out.push(k);
                        chars.next();
                    }
                }

                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }

    out
}

pub fn fit_width(input: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;

    for g in input.graphemes(true) {
        let g_width = UnicodeWidthStr::width(g);

        if used + g_width > width {
            break;
        }

        out.push_str(g);
        used += g_width;
    }

    // Pad if needed
    if used < width {
        out.extend(std::iter::repeat(' ').take(width - used));
    }

    out
}

/// Resolve escape sequences
pub fn resolve_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            consume_escaped(&mut chars, &mut out);
            continue;
        }
        out.push(c);
    }
    out
}
