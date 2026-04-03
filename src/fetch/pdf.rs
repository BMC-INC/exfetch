use thiserror::Error;

/// Errors that can occur during PDF text extraction.
#[derive(Debug, Error)]
pub enum PdfError {
    #[error("failed to parse PDF: {0}")]
    ParseError(String),

    #[error("PDF contains no extractable text")]
    NoText,
}

/// Extract text content from raw PDF bytes.
///
/// Loads the PDF from an in-memory buffer, iterates over all pages,
/// and concatenates extracted text. Returns `PdfError::NoText` if the
/// resulting text is empty after trimming whitespace.
pub fn extract_text(bytes: &[u8]) -> Result<String, PdfError> {
    let doc =
        lopdf::Document::load_mem(bytes).map_err(|e| PdfError::ParseError(e.to_string()))?;

    let pages = doc.get_pages();
    let mut all_text = String::new();

    // Pages are returned as BTreeMap<u32, ObjectId>; iterate in page order
    for (&page_num, _) in &pages {
        match doc.extract_text(&[page_num]) {
            Ok(text) => {
                if !all_text.is_empty() && !text.is_empty() {
                    all_text.push('\n');
                }
                all_text.push_str(&text);
            }
            Err(_) => {
                // Some pages may not have extractable text (e.g. scanned images);
                // skip them rather than failing the whole document.
                continue;
            }
        }
    }

    let trimmed = all_text.trim().to_string();
    if trimmed.is_empty() {
        return Err(PdfError::NoText);
    }

    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_bytes_returns_parse_error() {
        let garbage = b"this is definitely not a PDF";
        let result = extract_text(garbage);
        assert!(result.is_err());
        match result.unwrap_err() {
            PdfError::ParseError(_) => {} // expected
            other => panic!("expected ParseError, got: {:?}", other),
        }
    }

    #[test]
    fn test_extract_text_from_minimal_pdf() {
        // Build a minimal PDF in memory using lopdf
        use lopdf::dictionary;
        use lopdf::{Document, Object, Stream};

        let mut doc = Document::with_version("1.4");

        // Create a font dictionary entry
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });

        // Create a page content stream with text operators
        let content = b"BT /F1 12 Tf 100 700 Td (Hello exfetch PDF) Tj ET";
        let content_stream = Stream::new(dictionary! {}, content.to_vec());
        let content_id = doc.add_object(content_stream);

        // Resources dictionary referencing our font
        let resources = dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        };

        // Create a page
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
            "Resources" => resources,
        });

        // Pages node
        let pages_id = doc.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        });

        // Set parent on the page
        if let Ok(page) = doc.get_object_mut(page_id) {
            if let Object::Dictionary(ref mut dict) = page {
                dict.set("Parent", pages_id);
            }
        }

        // Catalog
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });

        doc.trailer.set("Root", catalog_id);

        // Save to bytes
        let mut buf = Vec::new();
        doc.save_to(&mut buf).expect("failed to save test PDF");

        // Now extract text
        let result = extract_text(&buf);
        assert!(result.is_ok(), "extraction failed: {:?}", result.err());
        let text = result.unwrap();
        assert!(
            text.contains("Hello exfetch PDF"),
            "expected 'Hello exfetch PDF' in extracted text, got: {:?}",
            text
        );
    }
}
