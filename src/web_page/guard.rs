//! Keeps chat-supplied links away from the bot's own network, such as the
//! Cobalt API, a local Telegram Bot API server, or cloud metadata endpoints.

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};
use tokio::net::lookup_host;
use url::{Host, Url};

/// Resolves host names to their public addresses only, so every connection,
/// including those after redirects, goes to a public address.
#[derive(Debug, Clone, Copy)]
pub struct PublicResolver;

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs = lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| is_public(addr.ip()))
                .collect::<Vec<SocketAddr>>();
            if addrs.is_empty() {
                let message = format!("{host} has no public address");
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, message).into());
            }
            let addrs: Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

/// Whether `url` is http(s) and does not name a non-public IP address.
/// Host names are checked when [`PublicResolver`] resolves them.
pub fn is_allowed_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(Host::Domain(_)) => true,
        Some(Host::Ipv4(ip)) => is_public(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_public(IpAddr::V6(ip)),
        None => false,
    }
}

/// Whether `ip` is reachable on the public internet.
pub const fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => is_public_v6(ip),
    }
}

const fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        // 0.0.0.0/8, "this network".
        || a == 0
        // 100.64.0.0/10, carrier-grade NAT.
        || (a == 100 && (b & 0xc0) == 64)
        // 192.0.0.0/24, IETF protocol assignments.
        || (a == 192 && b == 0 && c == 0)
        // 198.18.0.0/15, benchmarking.
        || (a == 198 && (b & 0xfe) == 18)
        // 240.0.0.0/4, reserved.
        || a >= 240)
}

const fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(ip) = ip.to_ipv4_mapped() {
        return is_public_v4(ip);
    }
    let segments = ip.segments();
    // 64:ff9b::/96, NAT64 translation of an IPv4 address.
    if let [0x64, 0xff9b, 0, 0, 0, 0, high, low] = segments {
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        return is_public_v4(Ipv4Addr::new(a, b, c, d));
    }
    let [first, second, ..] = segments;
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        // fc00::/7, unique local.
        || (first & 0xfe00) == 0xfc00
        // fe80::/10, link-local.
        || (first & 0xffc0) == 0xfe80
        // 2001:db8::/32, documentation.
        || (first == 0x2001 && second == 0x0db8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use rstest::rstest;

    #[rstest]
    #[case::public_v4("93.184.215.14")]
    #[case::public_v6("2606:4700::6810:84e5")]
    #[case::mapped_public_v4("::ffff:93.184.215.14")]
    #[case::nat64_public_v4("64:ff9b::5db8:d70e")]
    fn accepts_public_addresses(#[case] ip: &str) {
        assert!(is_public(assert_ok!(ip.parse())));
    }

    #[rstest]
    #[case::loopback("127.0.0.1")]
    #[case::private("10.1.2.3")]
    #[case::private_192("192.168.1.10")]
    #[case::metadata("169.254.169.254")]
    #[case::this_network("0.1.2.3")]
    #[case::cgnat("100.100.0.1")]
    #[case::benchmarking("198.19.0.1")]
    #[case::reserved("250.0.0.1")]
    #[case::v6_loopback("::1")]
    #[case::v6_unique_local("fd00::1")]
    #[case::v6_link_local("fe80::1")]
    #[case::mapped_loopback("::ffff:127.0.0.1")]
    #[case::nat64_private("64:ff9b::a00:1")]
    fn rejects_non_public_addresses(#[case] ip: &str) {
        assert!(!is_public(assert_ok!(ip.parse())));
    }

    #[rstest]
    #[case::domain("https://example.com/post", true)]
    #[case::plain_http("http://example.com/", true)]
    #[case::public_ip("http://93.184.215.14/", true)]
    #[case::loopback_ip("http://127.0.0.1:9000/", false)]
    #[case::v6_loopback_ip("http://[::1]/", false)]
    #[case::file("file:///etc/passwd", false)]
    #[case::ftp("ftp://example.com/", false)]
    fn allows_only_public_http_urls(#[case] url: &str, #[case] allowed: bool) {
        assert_eq!(is_allowed_url(&assert_ok!(Url::parse(url))), allowed);
    }
}
