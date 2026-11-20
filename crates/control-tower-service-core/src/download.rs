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
    use super::{push_checked, MAX_SUBSCRIPTION_BYTES};

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
