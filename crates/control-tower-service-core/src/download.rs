//! Bounded reading of subscription downloads.
//!
//! Subscription URLs come from the user (Web UI, CLI, cron jobs), so the server
//! must not trust their size. Reading the body with `Response::text()` buffers
//! whatever the remote sends; a broken or malicious endpoint could therefore
//! exhaust the service's memory. These helpers stop at [`MAX_SUBSCRIPTION_BYTES`]
//! and report a clear error instead.

use anyhow::{bail, Context, Result};

/// Maximum accepted subscription size.
///
/// Real subscriptions are a few hundred KiB at most (thousands of proxies), so
/// 10 MiB leaves plenty of headroom while keeping memory bounded.
pub const MAX_SUBSCRIPTION_BYTES: u64 = 10 * 1024 * 1024;

/// Quota and expiry reported by subscription providers in the
/// `subscription-userinfo` response header, e.g.
/// `upload=1234; download=5678; total=107374182400; expire=1767139200`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubscriptionUserInfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: i64,
}

/// Parse a `subscription-userinfo` header value.
///
/// Providers are inconsistent about separators (`;` and `,` are both seen) and
/// about key casing, so the parser is deliberately lenient. Returns `None`
/// when no known key is present, so a missing header can be distinguished from
/// a zeroed one.
pub fn parse_subscription_userinfo(value: &str) -> Option<SubscriptionUserInfo> {
    let mut info = SubscriptionUserInfo::default();
    let mut found = false;
    for part in value.split([';', ',']) {
        let Some((key, raw)) = part.split_once('=') else {
            continue;
        };
        let Ok(number) = raw.trim().parse::<i64>() else {
            continue;
        };
        let number = number.max(0);
        match key.trim().to_ascii_lowercase().as_str() {
            "upload" => {
                info.upload = number as u64;
                found = true;
            }
            "download" => {
                info.download = number as u64;
                found = true;
            }
            "total" => {
                info.total = number as u64;
                found = true;
            }
            "expire" => {
                info.expire = number;
                found = true;
            }
            _ => {}
        }
    }
    found.then_some(info)
}

/// Extract quota information from a subscription response.
pub fn userinfo_from_headers(
    headers: &reqwest::header::HeaderMap,
) -> Option<SubscriptionUserInfo> {
    headers
        .get("subscription-userinfo")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_subscription_userinfo)
}

fn push_checked(body: &mut Vec<u8>, chunk: &[u8], limit: u64) -> Result<()> {
    if body.len() as u64 + chunk.len() as u64 > limit {
        bail!("Downloaded content exceeds the {} byte limit", limit);
    }
    body.extend_from_slice(chunk);
    Ok(())
}

/// Read an async `reqwest::Response` body as UTF-8 text, refusing bodies larger
/// than `limit` bytes.
pub async fn text_limited(mut response: reqwest::Response, limit: u64) -> Result<String> {
    if let Some(len) = response.content_length() {
        if len > limit {
            bail!(
                "Downloaded content is too large ({} bytes, limit {} bytes)",
                len,
                limit
            );
        }
    }

    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Failed to read response body")?
    {
        push_checked(&mut body, &chunk, limit)?;
    }

    String::from_utf8(body).context("Downloaded content is not valid UTF-8")
}

/// Blocking variant for download code that runs inside `spawn_blocking`.
pub fn text_limited_blocking(
    response: reqwest::blocking::Response,
    limit: u64,
) -> Result<String> {
    use std::io::Read;

    if let Some(len) = response.content_length() {
        if len > limit {
            bail!(
                "Downloaded content is too large ({} bytes, limit {} bytes)",
                len,
                limit
            );
        }
    }

    // Read one byte past the limit so an oversized stream is detected without
    // buffering more than `limit + 1` bytes.
    let mut body = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut body)
        .context("Failed to read response body")?;
    if body.len() as u64 > limit {
        bail!("Downloaded content exceeds the {} byte limit", limit);
    }

    String::from_utf8(body).context("Downloaded content is not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::{parse_subscription_userinfo, push_checked, MAX_SUBSCRIPTION_BYTES};

    #[test]
    fn subscription_userinfo_parses_semicolon_format() {
        let info = parse_subscription_userinfo(
            "upload=1073741824; download=5368709120; total=107374182400; expire=1767139200",
        )
        .unwrap();
        assert_eq!(info.upload, 1073741824);
        assert_eq!(info.download, 5368709120);
        assert_eq!(info.total, 107374182400);
        assert_eq!(info.expire, 1767139200);
    }

    #[test]
    fn subscription_userinfo_accepts_commas_and_case() {
        let info =
            parse_subscription_userinfo("Upload=1, Download=2, Total=3, Expire=4").unwrap();
        assert_eq!((info.upload, info.download, info.total, info.expire), (1, 2, 3, 4));
    }

    #[test]
    fn subscription_userinfo_ignores_unknown_and_bad_values() {
        assert!(parse_subscription_userinfo("foo=bar").is_none());
        let info = parse_subscription_userinfo("upload=-5; expire=abc; total=10").unwrap();
        assert_eq!(info.upload, 0);
        assert_eq!(info.expire, 0);
        assert_eq!(info.total, 10);
    }

    #[test]
    fn chunked_body_stops_at_the_limit() {
        let mut body = Vec::new();
        push_checked(&mut body, b"abc", 4).unwrap();
        assert!(push_checked(&mut body, b"de", 4).is_err());
        assert_eq!(body, b"abc");
    }

    #[test]
    fn default_limit_is_ten_mebibytes() {
        assert_eq!(MAX_SUBSCRIPTION_BYTES, 10 * 1024 * 1024);
    }
}
