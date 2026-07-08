//! Argument-template substitution (§3).
//!
//! - `{file}` — the local file path.
//! - `{arg}`  — expands to every positional `arg=` value, each as its own argv
//!   entry (only when it is the entire token).
//! - `{key}`  — any other named param substitutes by key inside a token.

use std::collections::HashMap;

/// Build the concrete argv from a template.
///
/// `file` is the resolved local path (relay temp file, or a `run` route's
/// `file` named param). `named` holds the remaining named params. `positional`
/// holds the ordered `arg=` values used to expand a standalone `{arg}` token.
pub fn build_argv(
    template: &[String],
    file: Option<&str>,
    named: &HashMap<String, String>,
    positional: &[String],
) -> Vec<String> {
    let mut out = Vec::with_capacity(template.len());

    for token in template {
        if token == "{arg}" {
            // Standalone {arg} expands to N separate argv entries.
            out.extend(positional.iter().cloned());
            continue;
        }
        out.push(substitute_token(token, file, named));
    }

    out
}

/// Replace `{file}` and `{key}` placeholders within a single token. Unknown
/// placeholders are left intact so misconfiguration is visible rather than
/// silently dropped.
fn substitute_token(token: &str, file: Option<&str>, named: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(token.len());
    let mut chars = token.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if c != '{' {
            result.push(c);
            continue;
        }
        // Find the matching '}'.
        if let Some(end) = token[i + 1..].find('}') {
            let key = &token[i + 1..i + 1 + end];
            let replacement = if key == "file" {
                file.map(|f| f.to_string())
            } else {
                named.get(key).cloned()
            };

            match replacement {
                Some(value) => {
                    result.push_str(&value);
                    // Advance the iterator past the consumed `{key}`.
                    for _ in 0..(end + 1) {
                        chars.next();
                    }
                }
                None => {
                    // Unknown placeholder: keep it literally.
                    result.push(c);
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn expands_file_and_arg() {
        let template = vec!["{file}".to_string(), "{arg}".to_string()];
        let argv = build_argv(
            &template,
            Some("/tmp/x.txt"),
            &named(&[]),
            &["--wait".to_string(), "-n".to_string()],
        );
        assert_eq!(argv, vec!["/tmp/x.txt", "--wait", "-n"]);
    }

    #[test]
    fn substitutes_named_within_token() {
        let template = vec!["--out={dir}/result".to_string(), "{file}".to_string()];
        let argv = build_argv(&template, Some("/f"), &named(&[("dir", "/o")]), &[]);
        assert_eq!(argv, vec!["--out=/o/result", "/f"]);
    }

    #[test]
    fn keeps_unknown_placeholder() {
        let template = vec!["{nope}".to_string()];
        let argv = build_argv(&template, None, &named(&[]), &[]);
        assert_eq!(argv, vec!["{nope}"]);
    }
}
