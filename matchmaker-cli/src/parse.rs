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

pub fn get_pairs(pairs: Vec<String>) -> Result<Vec<(ArrayVec<String, 10>, String)>, ParseError> {
    let mut result = Vec::new();
    let mut iter = pairs.into_iter().peekable();

    while let Some(item) = iter.next() {
        let (path_str, value) = if let Some(eq_pos) = item.find('=') {
            // path=value
            let path = item[..eq_pos].to_string();
            let val = item[eq_pos + 1..].to_string();
            if val.is_empty() {
                return Err(ParseError::MissingValue { path: path.clone() });
            }
            (path, val)
        } else {
            // path value
            let path = item;
            let val = iter
                .next()
                .ok_or_else(|| ParseError::MissingValue { path: path.clone() })?;
            (path, val)
        };

        let mut components = ArrayVec::<String, 10>::new();
        for comp in path_str.split('.') {
            if comp.is_empty() || !comp.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
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

pub fn try_split_kv(vec: &mut Vec<String>) -> anyhow::Result<()> {
    fn valid_key(s: &str) -> bool {
        !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_')
    }

    // Check first element for '='
    if let Some(first) = vec.first() {
        if let Some(pos) = first.find('=') {
            let key = &first[..pos];
            // If the first element is a valid k=v pair, split the rest, and require that they succeed
            if valid_key(key) {
                let mut out = Vec::with_capacity(vec.len() * 2);
                for s in vec.iter() {
                    if let Some(pos) = s.find('=') {
                        let key = &s[..pos];
                        let val = &s[pos + 1..];
                        if !valid_key(key) {
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
        try_split_kv(&mut v).unwrap();
        assert_eq!(v, vec!["foo", "bar", "baz", "qux"]);

        // No split (no '=' in first element), unchanged
        let mut v2 = vec_!["hello", "world"];
        try_split_kv(&mut v2).unwrap();
        assert_eq!(v2, vec!["hello", "world"]);

        // No split (first element key invalid), unchanged
        let mut v3 = vec_!["NotAKey=val"];
        try_split_kv(&mut v3).unwrap();
        assert_eq!(v3, vec!["NotAKey=val"]);
    }

    #[test]
    fn test_invalid_key_in_split() {
        let mut v4 = vec_![
            "key=value",    // valid first element → triggers splitting
            "NotKey=value", // invalid key → should cause error
            "another_key=123",
        ];

        let err = try_split_kv(&mut v4).unwrap_err();
        assert_eq!(err.to_string(), "Invalid key: NotKey");

        // vec should remain unchanged
        assert_eq!(v4, vec_!["key=value", "NotKey=value", "another_key=123"]);
    }
}
