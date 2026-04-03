use crate::extract::markdown as extract_md;
use crate::output::text;

/// Convert HTML to Markdown, then optionally truncate.
pub fn format(html: &str, max_length: Option<usize>) -> String {
    let md = extract_md::to_markdown(html);
    text::format_raw(&md, max_length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_output_contains_content() {
        let html = "<html><body><main><h1>Hi</h1><p>World</p></main></body></html>";
        let out = format(html, None);
        assert!(out.contains("Hi"));
        assert!(out.contains("World"));
    }

    #[test]
    fn markdown_output_truncates() {
        let html = "<html><body><main><p>A long paragraph of text that should be truncated</p></main></body></html>";
        let out = format(html, Some(10));
        assert!(out.contains("[truncated at"));
    }
}
