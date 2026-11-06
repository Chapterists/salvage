//! Lenient JSON for hand-edited config files: accepts `//` and `/* */`
//! comments and trailing commas, which strict JSON rejects.

use serde_json::Value;

/// Parses strict JSON, falling back to a relaxed reading if that fails.
pub fn parse(text: &str) -> Result<Value, serde_json::Error> {
    match serde_json::from_str(text) {
        Ok(value) => Ok(value),
        Err(strict_error) => serde_json::from_str(&relax(text)).map_err(|_| strict_error),
    }
}

/// Removes comments and trailing commas outside of strings.
fn relax(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match (c, chars.get(i + 1)) {
            ('"', _) => {
                in_string = true;
                out.push(c);
                i += 1;
            }
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
            }
            (',', _) => {
                let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
                if !matches!(next, Some('}') | Some(']')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_json_passes_through() {
        let value = parse(r#"{"a": "b"}"#).unwrap();
        assert_eq!(value["a"], "b");
    }

    #[test]
    fn accepts_comments_and_trailing_commas() {
        let text = r#"{
            // line comment
            "url": "https://example.com/a//b", /* block */
            "list": [1, 2,],
        }"#;
        let value = parse(text).unwrap();
        assert_eq!(value["url"], "https://example.com/a//b");
        assert_eq!(value["list"][1], 2);
    }

    #[test]
    fn reports_real_errors() {
        assert!(parse("{ nope }").is_err());
    }
}
