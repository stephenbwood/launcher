//! Parsing of the `launcher://` URL scheme (§2).
//!
//! ```text
//! launcher://run/<app-id>?key=value&arg=positional1&arg=positional2
//! launcher://relay/<app-id>?src=<url>&dest=<url>&filename=report.docx
//! ```

use std::collections::HashMap;

use url::Url;

use crate::error::{AppError, AppResult};

/// A parsed `launcher://` request.
#[derive(Debug, Clone)]
pub enum Route {
    /// Launch an app with named/positional params (§2.1).
    Run {
        app_id: String,
        /// Named query params (everything except repeated `arg=`), URL-decoded.
        named: HashMap<String, String>,
        /// Positional `arg=` values, preserved in order (§2.1).
        positional: Vec<String>,
    },
    /// Download → edit → upload (§2.2).
    Relay {
        app_id: String,
        src: String,
        dest: String,
        filename: String,
    },
}

/// Parse a raw `launcher://…` string into a [`Route`].
pub fn parse(raw: &str) -> AppResult<Route> {
    let url = Url::parse(raw.trim()).map_err(|e| AppError::InvalidUrl(e.to_string()))?;

    if url.scheme() != "launcher" {
        return Err(AppError::InvalidUrl(format!(
            "expected scheme 'launcher', got '{}'",
            url.scheme()
        )));
    }

    // The route type is the authority/host component:  launcher://run/...
    let route_type = url
        .host_str()
        .ok_or_else(|| AppError::InvalidUrl("missing route type (run|relay)".into()))?
        .to_ascii_lowercase();

    // The app-id is the first non-empty path segment.
    let app_id = url
        .path_segments()
        .and_then(|mut segs| segs.find(|s| !s.is_empty()))
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::InvalidUrl("missing <app-id>".into()))?;

    match route_type.as_str() {
        "run" => {
            let mut named = HashMap::new();
            let mut positional = Vec::new();
            // query_pairs() yields already-percent-decoded (key, value) pairs.
            for (key, value) in url.query_pairs() {
                if key == "arg" {
                    positional.push(value.into_owned());
                } else {
                    named.insert(key.into_owned(), value.into_owned());
                }
            }
            Ok(Route::Run {
                app_id,
                named,
                positional,
            })
        }
        "relay" => {
            let mut src = None;
            let mut dest = None;
            let mut filename = None;
            for (key, value) in url.query_pairs() {
                match key.as_ref() {
                    "src" => src = Some(value.into_owned()),
                    "dest" => dest = Some(value.into_owned()),
                    "filename" => filename = Some(value.into_owned()),
                    _ => {}
                }
            }
            let src = src.ok_or_else(|| AppError::InvalidUrl("relay: missing 'src'".into()))?;
            let dest = dest.ok_or_else(|| AppError::InvalidUrl("relay: missing 'dest'".into()))?;
            let filename =
                filename.ok_or_else(|| AppError::InvalidUrl("relay: missing 'filename'".into()))?;

            // Guard against path traversal in the supplied filename (§6.1 —
            // the file lands inside the session dir under this name).
            let filename = sanitize_filename(&filename)?;

            Ok(Route::Relay {
                app_id,
                src,
                dest,
                filename,
            })
        }
        other => Err(AppError::InvalidUrl(format!(
            "unknown route type '{other}' (expected 'run' or 'relay')"
        ))),
    }
}

/// Reduce an arbitrary supplied filename to a safe basename, rejecting empty /
/// traversal-only inputs. Prevents `filename=../../etc/passwd` from escaping the
/// session directory.
fn sanitize_filename(name: &str) -> AppResult<String> {
    let base = name
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if base.is_empty() || base == "." || base == ".." {
        return Err(AppError::InvalidUrl(format!("invalid filename '{name}'")));
    }
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_run_with_named_and_positional() {
        let route = parse("launcher://run/vscode?file=/path/to/file.txt&arg=--wait&arg=-n").unwrap();
        match route {
            Route::Run {
                app_id,
                named,
                positional,
            } => {
                assert_eq!(app_id, "vscode");
                assert_eq!(named.get("file").unwrap(), "/path/to/file.txt");
                assert_eq!(positional, vec!["--wait", "-n"]);
            }
            _ => panic!("expected run route"),
        }
    }

    #[test]
    fn parses_relay() {
        let route =
            parse("launcher://relay/word?src=https://a/get&dest=https://b/put&filename=report.docx")
                .unwrap();
        match route {
            Route::Relay {
                app_id,
                src,
                dest,
                filename,
            } => {
                assert_eq!(app_id, "word");
                assert_eq!(src, "https://a/get");
                assert_eq!(dest, "https://b/put");
                assert_eq!(filename, "report.docx");
            }
            _ => panic!("expected relay route"),
        }
    }

    #[test]
    fn strips_traversal_from_filename() {
        let route = parse(
            "launcher://relay/word?src=https://a&dest=https://b&filename=../../etc/passwd",
        )
        .unwrap();
        match route {
            Route::Relay { filename, .. } => {
                assert_eq!(filename, "passwd");
                assert!(!filename.contains(".."));
            }
            _ => panic!("expected relay route"),
        }
    }

    #[test]
    fn rejects_dotdot_only_filename() {
        assert!(sanitize_filename("..").is_err());
        assert!(sanitize_filename("").is_err());
    }

    #[test]
    fn rejects_bad_scheme() {
        assert!(parse("https://run/vscode").is_err());
    }
}
