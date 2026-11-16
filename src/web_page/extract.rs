use scraper::{ElementRef, Html, Selector};

/// Article text kept from a page; the prompt truncates it further.
const MAX_TEXT_CHARS: usize = 4000;
/// Where the article usually is, most specific first.
const CONTAINERS: [&str; 4] = ["article", "main", "[role=main]", "body"];

/// What a page says about itself, as far as its HTML shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebPage {
    pub site_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Paragraphs of the article, collapsed to one line and capped at
    /// `MAX_TEXT_CHARS`.
    pub text: Option<String>,
}

impl WebPage {
    /// Read Open Graph and standard metadata, and the paragraphs of the
    /// first `article`, `main`, or `body` element that has any.
    pub fn parse(html: &str) -> Self {
        let document = Html::parse_document(html);
        Self {
            site_name: meta(&document, "property", "og:site_name"),
            title: meta(&document, "property", "og:title")
                .or_else(|| meta(&document, "name", "twitter:title"))
                .or_else(|| first_text(&document, "title")),
            description: meta(&document, "property", "og:description")
                .or_else(|| meta(&document, "name", "description"))
                .or_else(|| meta(&document, "name", "twitter:description")),
            text: article_text(&document),
        }
    }
}

fn meta(document: &Html, attribute: &str, key: &str) -> Option<String> {
    let selector = Selector::parse(&format!("meta[{attribute}=\"{key}\"]")).ok()?;
    document
        .select(&selector)
        .filter_map(|element| element.attr("content"))
        .find_map(one_line)
}

fn first_text(document: &Html, css: &str) -> Option<String> {
    let selector = Selector::parse(css).ok()?;
    document
        .select(&selector)
        .find_map(|element| one_line(&element.text().collect::<String>()))
}

fn article_text(document: &Html) -> Option<String> {
    let paragraph = Selector::parse("p").ok()?;
    CONTAINERS
        .iter()
        .filter_map(|css| Selector::parse(css).ok())
        .find_map(|container| {
            document
                .select(&container)
                .find_map(|element| paragraphs(element, &paragraph))
        })
}

/// The paragraphs inside `container`, joined until `MAX_TEXT_CHARS`.
fn paragraphs(container: ElementRef<'_>, paragraph: &Selector) -> Option<String> {
    let mut text = String::new();
    for element in container.select(paragraph) {
        let Some(line) = one_line(&element.text().collect::<String>()) else {
            continue;
        };
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&line);
        if text.chars().count() >= MAX_TEXT_CHARS {
            return Some(text.chars().take(MAX_TEXT_CHARS).collect());
        }
    }
    (!text.is_empty()).then_some(text)
}

fn one_line(text: &str) -> Option<String> {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then_some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some, assert_some_eq};

    #[test]
    fn prefers_open_graph_metadata_and_article_paragraphs() {
        let page = WebPage::parse(
            r#"<!doctype html>
            <html><head>
              <title>Fallback title</title>
              <meta property="og:site_name" content="The Race">
              <meta property="og:title" content="  Haas   upgrade explained ">
              <meta name="description" content="Plain description">
              <meta property="og:description" content="OG description">
            </head><body>
              <nav><p>Home</p></nav>
              <article>
                <h1>Haas upgrade</h1>
                <p>The new floor
                   arrives in Monza.</p>
                <p> </p>
                <p>It is <em>worth</em> two tenths.</p>
              </article>
              <script>trackEverything()</script>
            </body></html>"#,
        );

        assert_eq!(
            page,
            WebPage {
                site_name: Some("The Race".to_owned()),
                title: Some("Haas upgrade explained".to_owned()),
                description: Some("OG description".to_owned()),
                text: Some("The new floor arrives in Monza. It is worth two tenths.".to_owned()),
            }
        );
    }

    #[test]
    fn falls_back_to_standard_metadata_and_body_paragraphs() {
        let page = WebPage::parse(
            r#"<html><head>
              <title> My blog </title>
              <meta name="description" content="About things">
            </head><body><div><p>Only paragraph.</p></div></body></html>"#,
        );

        assert_some_eq!(page.title, "My blog");
        assert_some_eq!(page.description, "About things");
        assert_some_eq!(page.text, "Only paragraph.");
        assert_none!(page.site_name);
    }

    #[test]
    fn skips_an_article_without_paragraphs() {
        let page =
            WebPage::parse("<body><article><div>teaser</div></article><p>Body text.</p></body>");

        assert_some_eq!(page.text, "Body text.");
    }

    #[test]
    fn caps_article_text() {
        let paragraph = format!("<p>{}</p>", "word ".repeat(500));
        let page = WebPage::parse(&format!("<article>{}</article>", paragraph.repeat(5)));

        let text = assert_some!(page.text);
        assert_eq!(text.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn empty_page_has_nothing() {
        assert_eq!(WebPage::parse(""), WebPage::default());
    }
}
