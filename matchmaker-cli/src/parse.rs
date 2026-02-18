use anyhow::bail;
use matchmaker::action::ArrayVec;

use thiserror::Error;
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid path component '{component}' in path '{path}'")]
    InvalidPath { path: String, component: String },
    #[error("Missing value for path '{path}'")]
    MissingValue { path: String },
}

static ALIASES: &[(&str, &str)] = &[
    ("d", "matcher.columns.split"),
    ("i", "matcher.input_separator"),
    ("o", "matcher.output_template"),
    ("x", "matcher.command"),
    ("cmd", "matcher.command"),
];

/// Get (path, value) pairs by consuming either a single word, splitting at '=' into a valid key, or a pair of consecutive words.
/// The value is broken down into words, and fed into [`matchmaker_partial::Set`] to construct the (partial) type at `path`.
pub fn get_pairs(pairs: Vec<String>) -> Result<Vec<(ArrayVec<String, 10>, String)>, ParseError> {
    let mut result = Vec::new();
    let mut iter = pairs.into_iter().peekable();

    while let Some(item) = iter.next() {
        let (mut path_str, value) = if let Some(eq_pos) = item.find('=') {
            let path = item[..eq_pos].to_string();
            let val = item[eq_pos + 1..].to_string();
            if val.is_empty() {
                return Err(ParseError::MissingValue { path: path.clone() });
            }
            (path, val)
        } else {
            let path = item;
            let val = iter
                .next()
                .ok_or_else(|| ParseError::MissingValue { path: path.clone() })?;
            (path, val)
        };

        // Apply alias to full path string
        if let Some((_, expanded)) = ALIASES.iter().find(|(a, _)| *a == path_str) {
            path_str = (*expanded).to_string();
        }

        let mut components = ArrayVec::<String, 10>::new();
        for comp in path_str.split('.') {
            if !valid_key(comp, false) {
                return Err(ParseError::InvalidPath {
                    path: path_str.clone(),
                    component: comp.to_string(),
                });
            }
            components.push(comp.to_string());
        }

        result.push((components, value));
    }

    Ok(result)
}

pub fn valid_key(s: &str, extended: bool) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            (extended && c.is_ascii_graphic()) || c.is_ascii_lowercase() || "_".contains(c)
        })
}

/// Determine if a sequence of words should be interpreted as words representing key-value pairs (in which case they are split in two), or not (in which case the words are unchanged).
/// This is relevant to Maps and Structs as they are defined given a word sequences, interpreting it in word pairs.
pub fn try_split_kv(vec: &mut Vec<String>, extended_keys: bool) -> anyhow::Result<()> {
    // Check first element for '='
    if let Some(first) = vec.first()
        && let Some(pos) = first.find('=')
    {
        let key = &first[..pos];
        // If the first element is a valid k=v pair, split the rest, and require that they succeed
        if valid_key(key, extended_keys) {
            let mut out = Vec::with_capacity(vec.len() * 2);
            for s in vec.iter() {
                if let Some(pos) = s.find('=') {
                    let key = &s[..pos];
                    let val = &s[pos + 1..];
                    if !valid_key(key, extended_keys) {
                        bail!("Invalid key: {}", key);
                    }
                    out.push(key.to_string());
                    out.push(val.to_string());
                } else {
                    bail!("Expected '=' in element: {}", s);
                }
            }
            *vec = out;
        }
    }

    // otherwise no change
    Ok(())
}
#[cfg(test)]
mod tests {
    use cli_boilerplate_automation::vec_;

    use super::*;

    #[test]
    fn test_get_pairs() {
        // Valid input
        let input = vec_!["a.b.c=val1", "d.e", "val2", "f.g=val3",];
        let pairs = get_pairs(input).unwrap();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].0.as_slice(), &["a", "b", "c"]);
        assert_eq!(pairs[0].1, "val1");
        assert_eq!(pairs[1].0.as_slice(), &["d", "e"]);
        assert_eq!(pairs[1].1, "val2");
        assert_eq!(pairs[2].0.as_slice(), &["f", "g"]);
        assert_eq!(pairs[2].1, "val3");

        // Invalid path
        let input = vec_!["A.b=val"];
        let err = get_pairs(input).unwrap_err();
        match err {
            ParseError::InvalidPath { path, component } => {
                assert_eq!(path, "A.b");
                assert_eq!(component, "A");
            }
            _ => panic!("Expected InvalidPath"),
        }

        // Missing value
        let input = vec_!["a.b"];
        let err = get_pairs(input).unwrap_err();
        match err {
            ParseError::MissingValue { path } => {
                assert_eq!(path, "a.b");
            }
            _ => panic!("Expected MissingValue"),
        }

        // Empty input is allowed
        let input: Vec<String> = vec![];
        let pairs = get_pairs(input).unwrap();
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_split_key_values_in_place() {
        // Split occurs
        let mut v = vec_!["foo=bar", "baz=qux"];
        try_split_kv(&mut v, false).unwrap();
        assert_eq!(v, vec!["foo", "bar", "baz", "qux"]);

        // No split (no '=' in first element), unchanged
        let mut v2 = vec_!["hello", "world"];
        try_split_kv(&mut v2, false).unwrap();
        assert_eq!(v2, vec!["hello", "world"]);

        // No split (first element key invalid), unchanged
        let mut v3 = vec_!["Not AKey=val"];
        try_split_kv(&mut v3, false).unwrap();
        assert_eq!(v3, vec!["Not AKey=val"]);
    }

    #[test]
    fn test_invalid_key_in_split() {
        let mut v4 = vec_![
            "key=value",    // valid first element → triggers splitting
            "NotKey=value", // invalid key → should cause error
            "another_key=123",
        ];

        let err = try_split_kv(&mut v4, false).unwrap_err();
        assert_eq!(err.to_string(), "Invalid key: NotKey");

        // vec should remain unchanged
        assert_eq!(v4, vec_!["key=value", "NotKey=value", "another_key=123"]);
    }
}
