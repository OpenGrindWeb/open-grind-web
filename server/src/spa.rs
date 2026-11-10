//! Serves the SvelteKit build with the web bridge injected.
//!
//! Every HTML navigation gets `index.html` (SPA fallback) with
//! `<script src="/og-bridge.js">` inserted first in `<head>`, so the bridge
//! defines the Tauri globals before any app module runs. The front build
//! itself stays byte-for-byte the upstream one.

use std::path::Path;
use std::sync::Arc;

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

const BRIDGE_JS: &str = include_str!("bridge.js");
const BRIDGE_TAG: &str = r#"<script src="/og-bridge.js"></script>"#;

#[derive(Clone)]
pub struct Index(Arc<Option<String>>);

impl Index {
	pub fn load(static_dir: &Path) -> Self {
		let html = std::fs::read_to_string(static_dir.join("index.html"))
			.ok()
			.map(|html| inject(&html));
		Self(Arc::new(html))
	}

	pub fn response(&self) -> Response {
		match self.0.as_ref() {
			Some(html) => (
				[
					(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8")),
					(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
				],
				html.clone(),
			)
				.into_response(),
			None => (StatusCode::SERVICE_UNAVAILABLE, "front build missing").into_response(),
		}
	}
}

pub fn inject(html: &str) -> String {
	let after_head = html
		.find("<head")
		.and_then(|start| html[start..].find('>').map(|end| start + end + 1));
	match after_head {
		Some(at) => format!("{}{BRIDGE_TAG}{}", &html[..at], &html[at..]),
		None => format!("{BRIDGE_TAG}{html}"),
	}
}

pub async fn bridge() -> Response {
	(
		[
			(header::CONTENT_TYPE, HeaderValue::from_static("text/javascript; charset=utf-8")),
			(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
		],
		BRIDGE_JS,
	)
		.into_response()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_bridge_comes_first_in_head() {
		let html = r#"<!doctype html><html lang="en"><head data-x="1"><meta charset="utf-8"><script type="module">start()</script></head></html>"#;
		let out = inject(html);
		let bridge = out.find(BRIDGE_TAG).unwrap();
		assert!(bridge > out.find("<head").unwrap());
		assert!(bridge < out.find("<meta").unwrap());
		assert!(bridge < out.find("type=\"module\"").unwrap());
	}
}
