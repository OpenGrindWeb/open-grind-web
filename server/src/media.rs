//! Minimal port of the `ogmedia://` proxy (`src-tauri/src/media`).
//!
//! The front builds media URLs with `convertFileSrc("<tag><base64url(url)>",
//! "ogmedia")`; the bridge maps that to `/media/ogmedia/<path>`. The server
//! fetches through the Grindr client (same headers and TLS fingerprint as the
//! app; `grindr` itself refuses hosts that aren't a Grindr CDN or GIPHY) and
//! lets the browser cache the result.
//!
//! Upstream's in-process cache, request coalescing and fetcher fallbacks are
//! not ported: the browser cache covers the common case.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use grindr::{MediaFetcher, MediaRequest};

use crate::AppState;

const SCHEME: &str = "ogmedia";
const MAX_MEDIA_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub struct Target {
	pub url: String,
	pub fetcher: MediaFetcher,
}

/// Same format as `media/target.rs` upstream, without the leading slash.
pub fn decode_target(tagged: &str) -> Option<Target> {
	let (tag, encoded) = tagged.split_at_checked(1)?;
	let fetcher = match tag {
		"i" => MediaFetcher::ImageLoader,
		"v" => MediaFetcher::MediaPlayer,
		_ => return None,
	};
	let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
	let url = String::from_utf8(decoded).ok()?;
	url.starts_with("https://").then_some(Target { url, fetcher })
}

fn host_of(url: &str) -> &str {
	url.split_once("://")
		.map(|(_, rest)| rest.split('/').next().unwrap_or(rest))
		.unwrap_or("unknown")
}

pub async fn handler(
	State(app): State<AppState>,
	Path((scheme, path)): Path<(String, String)>,
	headers: HeaderMap,
) -> Response {
	if scheme != SCHEME {
		return StatusCode::NOT_FOUND.into_response();
	}
	// The front adds `?retry=N` on retries; axum's Path excludes the query.
	let Some(target) = decode_target(&path) else {
		return StatusCode::BAD_REQUEST.into_response();
	};
	let Ok(client) = app.client() else {
		return StatusCode::SERVICE_UNAVAILABLE.into_response();
	};
	let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());

	let media = match client
		.fetch_media(MediaRequest {
			url: &target.url,
			range,
			max_bytes: MAX_MEDIA_BYTES,
			fetcher: target.fetcher,
		})
		.await
	{
		Ok(media) => media,
		Err(e) => {
			// Never log the full url: chat media urls carry a signature.
			tracing::warn!("[media] fetch from {} failed: {}", host_of(&target.url), e);
			let status = match e {
				grindr::GrindrError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
				_ => StatusCode::BAD_GATEWAY,
			};
			return status.into_response();
		}
	};

	let status = StatusCode::from_u16(media.status).unwrap_or(StatusCode::BAD_GATEWAY);
	let mut response = Response::builder().status(status);
	let set = |builder: axum::http::response::Builder, name, value: Option<&String>| match value
		.and_then(|v| HeaderValue::from_str(v).ok())
	{
		Some(v) => builder.header(name, v),
		None => builder,
	};
	response = set(response, header::CONTENT_TYPE, media.content_type.as_ref());
	response = set(response, header::CONTENT_RANGE, media.content_range.as_ref());
	response = set(response, header::ACCEPT_RANGES, media.accept_ranges.as_ref());
	if status.is_success() {
		// Media urls are content-addressed (hash or signed url), so a cached
		// copy never goes stale. `private`: never in a shared proxy cache.
		response = response.header(header::CACHE_CONTROL, "private, max-age=604800, immutable");
	}
	response
		.body(Body::from(media.body))
		.unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn decodes_what_the_front_encodes() {
		let url = "https://cdns.grindr.com/images/thumb/320x320/abc";
		let target = decode_target(&format!("i{}", URL_SAFE_NO_PAD.encode(url))).unwrap();
		assert_eq!(target.url, url);
		assert_eq!(target.fetcher, MediaFetcher::ImageLoader);
		let video = decode_target(&format!("v{}", URL_SAFE_NO_PAD.encode(url))).unwrap();
		assert_eq!(video.fetcher, MediaFetcher::MediaPlayer);
	}

	#[test]
	fn refuses_non_https_and_unknown_tags() {
		assert!(decode_target(&format!("i{}", URL_SAFE_NO_PAD.encode("http://x/y"))).is_none());
		assert!(decode_target(&format!("x{}", URL_SAFE_NO_PAD.encode("https://x/y"))).is_none());
		assert!(decode_target("").is_none());
		assert!(decode_target("inot base64!").is_none());
	}

	#[test]
	fn host_hides_the_signed_query() {
		assert_eq!(host_of("https://a.cloudfront.net/x.jpg?Signature=s"), "a.cloudfront.net");
	}
}
