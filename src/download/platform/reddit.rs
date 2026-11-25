use crate::{
    download::{
        DownloadResult,
        error::{DownloadError, Result},
        platform::cobalt::{CobaltClient, safe_filename},
    },
    telemetry::ErrorChain,
};
use reqwest::{Client, Response, redirect::Policy};
use serde::Deserialize;
use std::time::Duration;
use tempfile::tempdir;
use tokio::fs;
use tracing::{debug, warn};
use url::Url;

const USER_AGENT: &str = concat!("guenther/", env!("CARGO_PKG_VERSION"));
const MAX_REDIRECTS: usize = 10;

/// Downloads Reddit posts through Cobalt, falling back to the post's embed
/// page for image posts.
///
/// Cobalt only handles Reddit videos and GIFs, and Reddit blocks its JSON API
/// for unauthenticated clients, while `embed.reddit.com` still serves post
/// data (as of 2026-11-25).
///
/// Requests stay on Reddit hosts, since post metadata picks the image URL.
#[derive(Debug, Clone)]
pub(super) struct RedditClient {
    http: Client,
}

impl RedditClient {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .redirect(Policy::custom(|attempt| {
                    if attempt.previous().len() < MAX_REDIRECTS && is_reddit_host(attempt.url()) {
                        attempt.follow()
                    } else {
                        attempt.stop()
                    }
                }))
                .build()
                .map_err(DownloadError::BuildHttpClient)?,
        })
    }

    pub(super) async fn download(
        &self,
        cobalt: &CobaltClient,
        url: &str,
    ) -> Result<DownloadResult> {
        let cobalt_error = match cobalt.download(url).await {
            Ok(result) => return Ok(result),
            Err(err) => err,
        };

        let fallback = match self.post_image_url(url).await {
            Ok(image_url) => self.download_image(&image_url).await,
            Err(err) => Err(err),
        };

        match fallback {
            Ok(result) => {
                debug!(
                    cobalt_error = %ErrorChain(&cobalt_error),
                    "Cobalt could not fetch Reddit media; used the embed image"
                );
                Ok(result)
            }
            Err(fallback_error) => {
                warn!(
                    fallback_error = %ErrorChain(&fallback_error),
                    "Reddit image fallback was not applicable"
                );
                Err(cobalt_error)
            }
        }
    }

    /// The image URL of an image post, read from its embed page.
    async fn post_image_url(&self, url: &str) -> Result<Url> {
        let mut post_url = Url::parse(url).map_err(|_| DownloadError::InvalidRedditUrl)?;
        if is_share_link(&post_url) {
            post_url = self.resolve_share_link(post_url).await?;
        }
        let embed_url = embed_url(&post_url).ok_or(DownloadError::InvalidRedditUrl)?;

        let html = self
            .http
            .get(embed_url)
            .send()
            .await
            .and_then(Response::error_for_status)
            .map_err(DownloadError::FetchRedditPost)?
            .text()
            .await
            .map_err(DownloadError::FetchRedditPost)?;
        let post = embedded_post(&html).ok_or(DownloadError::ParseRedditPost)?;
        post.image_url().ok_or(DownloadError::MissingRedditImage)
    }

    /// Follow a `/r/<sub>/s/<id>` share link to the post URL it redirects to.
    ///
    /// The post page itself may answer with an error to bots, so only the
    /// final URL is used.
    async fn resolve_share_link(&self, url: Url) -> Result<Url> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(DownloadError::FetchRedditPost)?;
        Ok(response.url().clone())
    }

    async fn download_image(&self, image_url: &Url) -> Result<DownloadResult> {
        let bytes = self
            .http
            .get(image_url.clone())
            .send()
            .await
            .and_then(Response::error_for_status)
            .map_err(DownloadError::DownloadRedditImage)?
            .bytes()
            .await
            .map_err(DownloadError::DownloadRedditImage)?;

        let tempdir = tempdir()?;
        let filename = image_url
            .path_segments()
            .and_then(Iterator::last)
            .unwrap_or_default();
        let path = tempdir.path().join(safe_filename(filename, 0));
        fs::write(&path, &bytes).await?;
        debug!(bytes = bytes.len(), "Downloaded Reddit image");

        Ok(DownloadResult {
            tempdir,
            files: vec![path],
            source_text: None,
        })
    }
}

fn is_reddit_host(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        ["reddit.com", "redd.it"].into_iter().any(|domain| {
            host == domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|sub| sub.ends_with('.'))
        })
    })
}

fn is_share_link(url: &Url) -> bool {
    let segments = url
        .path_segments()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    matches!(segments.as_slice(), ["r", _, "s", _, ..])
}

/// The `embed.reddit.com` page of a post URL, which needs the subreddit or
/// user in its path.
fn embed_url(post_url: &Url) -> Option<Url> {
    let segments = post_url
        .path_segments()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let [kind @ ("r" | "user"), owner, "comments", id, ..] = segments.as_slice() else {
        return None;
    };
    Url::parse(&format!(
        "https://embed.reddit.com/{kind}/{owner}/comments/{id}/"
    ))
    .ok()
}

#[derive(Debug, Deserialize)]
struct ScreenviewData {
    post: EmbeddedPost,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct EmbeddedPost {
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

impl EmbeddedPost {
    /// The image of an image post hosted on `i.redd.it`.
    fn image_url(&self) -> Option<Url> {
        if self.kind != "image" {
            return None;
        }
        Url::parse(&self.url)
            .ok()
            .filter(|url| url.scheme() == "https" && url.host_str() == Some("i.redd.it"))
    }
}

/// The post summary that the embed page puts in its
/// `<shreddit-screenview-data data="...">` analytics element.
fn embedded_post(html: &str) -> Option<EmbeddedPost> {
    const MARKER: &str = r#"<shreddit-screenview-data data=""#;
    let (_, rest) = html.split_once(MARKER)?;
    let (data, _) = rest.split_once('"')?;
    serde_json::from_str::<ScreenviewData>(&unescape_attribute(data))
        .ok()
        .map(|data| data.post)
}

/// Decode the entities that HTML serializers use in double-quoted attributes.
fn unescape_attribute(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok, assert_some_eq};
    use rstest::rstest;

    fn url(raw: &str) -> Url {
        assert_ok!(Url::parse(raw))
    }

    #[rstest]
    #[case::share("https://www.reddit.com/r/formuladank/s/TyBtJCUyRz", true)]
    #[case::post(
        "https://www.reddit.com/r/formuladank/comments/1y23htn/hear_me_out/",
        false
    )]
    #[case::video("https://v.redd.it/abc123", false)]
    fn detects_share_links(#[case] raw: &str, #[case] expected: bool) {
        assert_eq!(is_share_link(&url(raw)), expected);
    }

    #[rstest]
    #[case::subreddit(
        "https://www.reddit.com/r/formuladank/comments/1y23htn/hear_me_out/?share_id=x&utm_source=share",
        "https://embed.reddit.com/r/formuladank/comments/1y23htn/"
    )]
    #[case::comment(
        "https://old.reddit.com/r/formula1/comments/abc/title/comment/def/",
        "https://embed.reddit.com/r/formula1/comments/abc/"
    )]
    #[case::user(
        "https://www.reddit.com/user/someone/comments/abc/",
        "https://embed.reddit.com/user/someone/comments/abc/"
    )]
    fn builds_embed_url(#[case] raw: &str, #[case] expected: &str) {
        assert_some_eq!(embed_url(&url(raw)), url(expected));
    }

    #[rstest]
    #[case::bare_comments("https://www.reddit.com/comments/1y23htn")]
    #[case::share("https://www.reddit.com/r/formuladank/s/TyBtJCUyRz")]
    #[case::video("https://v.redd.it/abc123")]
    fn rejects_embed_url_without_post_owner(#[case] raw: &str) {
        assert_none!(embed_url(&url(raw)));
    }

    #[test]
    fn reads_image_post_from_embed_page() {
        let html = concat!(
            r#"<shreddit-app><shreddit-screenview-data data="{&quot;post&quot;:"#,
            r#"{&quot;id&quot;:&quot;t3_1y23htn&quot;,&quot;url&quot;:&quot;https://i.redd.it/7l4cyz5za03i1.png&quot;,"#,
            r#"&quot;title&quot;:&quot;Tom &amp; Jerry&#39;s &lt;3&quot;,&quot;type&quot;:&quot;image&quot;},"#,
            r#"&quot;subreddit&quot;:{&quot;name&quot;:&quot;formuladank&quot;}}"></shreddit-screenview-data>"#,
            r#"<shreddit-screenview-data data="{&quot;post&quot;:{&quot;type&quot;:&quot;video&quot;,&quot;url&quot;:&quot;x&quot;}}">"#,
        );

        assert_some_eq!(
            embedded_post(html),
            EmbeddedPost {
                kind: "image".into(),
                url: "https://i.redd.it/7l4cyz5za03i1.png".into(),
            }
        );
    }

    #[test]
    fn ignores_pages_without_post_data() {
        assert_none!(embedded_post("<html><body>Blocked</body></html>"));
        assert_none!(embedded_post(
            r#"<shreddit-screenview-data data="{&quot;subreddit&quot;:{}}">"#
        ));
    }

    #[rstest]
    #[case::image("image", "https://i.redd.it/7l4cyz5za03i1.png", true)]
    #[case::video("video", "https://v.redd.it/abc123", false)]
    #[case::other_host("image", "https://example.com/a.png", false)]
    #[case::plain_http("image", "http://i.redd.it/a.png", false)]
    fn accepts_only_reddit_hosted_images(
        #[case] kind: &str,
        #[case] url: &str,
        #[case] accepted: bool,
    ) {
        let post = EmbeddedPost {
            kind: kind.into(),
            url: url.into(),
        };
        assert_eq!(post.image_url().is_some(), accepted);
    }

    #[rstest]
    #[case::www("https://www.reddit.com/r/f1/comments/abc/", true)]
    #[case::apex("https://reddit.com/r/f1/s/abc", true)]
    #[case::image("https://i.redd.it/a.png", true)]
    #[case::lookalike("https://notreddit.com/r/f1/comments/abc/", false)]
    #[case::other("https://example.com/", false)]
    fn recognizes_reddit_hosts(#[case] raw: &str, #[case] expected: bool) {
        assert_eq!(is_reddit_host(&url(raw)), expected);
    }

    #[test]
    fn unescapes_ampersand_last() {
        assert_eq!(unescape_attribute("&amp;quot; &quot;"), "&quot; \"");
    }
}
