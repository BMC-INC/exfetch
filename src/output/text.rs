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

    // Byte limits may fall inside a multi-byte UTF-8 character.
    let mut boundary = max;
    while !body.is_char_boundary(boundary) {
        boundary -= 1;
    }
    // Find the last space at or before the valid boundary.
    let cut = match body[..boundary].rfind(' ') {
        Some(pos) => pos,
        None => boundary,
    };

    let truncated = &body[..cut];
    format!(
        "{}\n[truncated at {} bytes, full content {} bytes]",
        truncated,
        cut,
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_utf8_boundaries() {
        for limit in 0.."你好🌍".len() {
            let output = format_raw("你好🌍", Some(limit));
            let prefix = output.split('\n').next().unwrap();
            assert!(prefix.len() <= limit);
            assert!("你好🌍".starts_with(prefix));
        }
    }

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
