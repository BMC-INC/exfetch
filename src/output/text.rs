use crate::extract::readability;

/// Extract readable text from HTML, then optionally truncate.
pub fn format(html: &str, max_length: Option<usize>) -> String {
    let text = readability::extract(html);
    format_raw(&text, max_length)
}

/// Truncate a string at a word boundary. If truncation occurs, append a
/// marker showing where truncation happened and the full content length.
pub fn format_raw(body: &str, max_length: Option<usize>) -> String {
    let max = match max_length {
        Some(m) if m < body.len() => m,
        _ => return body.to_string(),
    };

    // Find the last space at or before `max` to avoid splitting a word.
    let cut = match body[..max].rfind(' ') {
        Some(pos) => pos,
        None => max, // no space found — cut at the hard limit
    };

    let truncated = &body[..cut];
    format!(
        "{}\n[truncated at {} chars, full content {} chars]",
        truncated,
        cut,
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_truncation_when_under_limit() {
        let out = format_raw("hello world", Some(100));
        assert_eq!(out, "hello world");
    }

    #[test]
    fn no_truncation_when_none() {
        let out = format_raw("hello world", None);
        assert_eq!(out, "hello world");
    }

    #[test]
    fn truncates_at_word_boundary() {
        let out = format_raw("hello wonderful world", Some(14));
        assert!(out.starts_with("hello"));
        assert!(out.contains("[truncated at"));
        // Should not cut in the middle of "wonderful"
        assert!(!out.contains("wonderf\n"));
    }
}
