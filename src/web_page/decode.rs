use encoding_rs::{Encoding, UTF_8};
use regex::Regex;

/// How far into the page browsers look for a `<meta>` charset declaration.
const META_PRESCAN_BYTES: usize = 1024;

/// Decode `html` by the charset in `content_type`, else by a `<meta>`
/// declaration near its start, else as UTF-8. A byte order mark overrides
/// both, as in browsers.
pub fn decode_html(html: &[u8], content_type: &str) -> String {
    let encoding = header_encoding(content_type)
        .or_else(|| meta_encoding(html))
        .unwrap_or(UTF_8);
    let (text, _, _) = encoding.decode(html);
    text.into_owned()
}

fn header_encoding(content_type: &str) -> Option<&'static Encoding> {
    let label = first_capture(r#"(?i)charset\s*=\s*["']?([\w.:-]+)"#, content_type)?;
    Encoding::for_label(label.as_bytes())
}

/// The charset of `<meta charset>` or `<meta http-equiv="Content-Type">`.
fn meta_encoding(html: &[u8]) -> Option<&'static Encoding> {
    let start = html.get(..META_PRESCAN_BYTES).unwrap_or(html);
    // Charset labels and the markup around them are ASCII in every encoding
    // a page can declare this way.
    let start = String::from_utf8_lossy(start);
    let label = first_capture(r#"(?i)<meta[^>]*charset\s*=\s*["']?([\w.:-]+)"#, &start)?;
    // A page cannot be UTF-16 if its ASCII markup was readable, so such
    // declarations mean UTF-8.
    Encoding::for_label(label.as_bytes()).map(Encoding::output_encoding)
}

fn first_capture(pattern: &str, text: &str) -> Option<String> {
    let regex = Regex::new(pattern).ok()?;
    let capture = regex.captures(text)?.get(1)?;
    Some(capture.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// "Привет" in Windows-1251.
    const CYRILLIC_1251: &[u8] = &[0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2];

    fn page(head: &str, body: &[u8]) -> Vec<u8> {
        let mut html = format!("<html><head>{head}</head><body><p>").into_bytes();
        html.extend_from_slice(body);
        html.extend_from_slice(b"</p></body></html>");
        html
    }

    #[rstest]
    #[case::header_charset("text/html; charset=windows-1251", "")]
    #[case::quoted_header_charset("text/html; Charset=\"cp1251\"", "")]
    #[case::meta_charset("text/html", "<meta charset=\"windows-1251\">")]
    #[case::meta_http_equiv(
        "text/html",
        "<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1251\">"
    )]
    fn decodes_declared_charset(#[case] content_type: &str, #[case] head: &str) {
        let html = decode_html(&page(head, CYRILLIC_1251), content_type);

        assert!(html.contains("<p>Привет</p>"), "{html}");
    }

    #[test]
    fn header_charset_wins_over_meta() {
        let html = decode_html(
            &page("<meta charset=\"iso-8859-1\">", CYRILLIC_1251),
            "text/html; charset=windows-1251",
        );

        assert!(html.contains("<p>Привет</p>"), "{html}");
    }

    #[test]
    fn byte_order_mark_wins_over_declarations() {
        let mut html = vec![0xef, 0xbb, 0xbf];
        html.extend(page("<meta charset=\"windows-1251\">", "Привет".as_bytes()));

        let html = decode_html(&html, "text/html; charset=windows-1251");
        assert!(html.contains("<p>Привет</p>"), "{html}");
    }

    #[rstest]
    #[case::undeclared("text/html", "")]
    #[case::unknown_label("text/html; charset=klingon", "")]
    #[case::utf16_meta("text/html", "<meta charset=\"utf-16\">")]
    fn defaults_to_utf8(#[case] content_type: &str, #[case] head: &str) {
        let html = decode_html(&page(head, "Grüße".as_bytes()), content_type);

        assert!(html.contains("<p>Grüße</p>"), "{html}");
    }
}
