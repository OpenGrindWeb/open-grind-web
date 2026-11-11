//! "Add to Home Screen" support: web app manifest, icons and the Apple meta
//! tags injected into every page. The icons are upstream's own iOS app icons
//! (opaque, as iOS expects), compiled in from `src-tauri/icons`.
//!
//! These routes are reachable without a gate session: iOS fetches the icon
//! and manifest on its own when the page is added, and none of it is private.

use axum::extract::Path;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub const HEAD_TAGS: &str = concat!(
	r#"<link rel="manifest" href="/manifest.webmanifest">"#,
	r#"<link rel="apple-touch-icon" href="/og-icons/apple-touch-icon.png">"#,
	r#"<meta name="apple-mobile-web-app-capable" content="yes">"#,
	r#"<meta name="mobile-web-app-capable" content="yes">"#,
	r#"<meta name="apple-mobile-web-app-title" content="Open Grind">"#,
	r#"<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">"#,
	r##"<meta name="theme-color" content="#0a0a0a">"##,
	r#"<meta name="format-detection" content="telephone=no">"#,
);

const MANIFEST: &str = r##"{
	"name": "Open Grind",
	"short_name": "Open Grind",
	"id": "/",
	"start_url": "/",
	"scope": "/",
	"display": "standalone",
	"orientation": "portrait",
	"background_color": "#0a0a0a",
	"theme_color": "#0a0a0a",
	"icons": [
		{ "src": "/og-icons/apple-touch-icon.png", "sizes": "180x180", "type": "image/png" },
		{ "src": "/og-icons/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any" },
		{ "src": "/og-icons/icon-1024.png", "sizes": "1024x1024", "type": "image/png", "purpose": "any" }
	]
}"##;

const APPLE_TOUCH_ICON: &[u8] = include_bytes!("../../src-tauri/icons/ios/AppIcon-60x60@3x.png");
const ICON_512: &[u8] = include_bytes!("../../src-tauri/icons/icon.png");
const ICON_1024: &[u8] = include_bytes!("../../src-tauri/icons/ios/AppIcon-512@2x.png");

pub fn is_public(path: &str) -> bool {
	path == "/manifest.webmanifest" || path.starts_with("/og-icons/") || path == "/favicon.ico"
}

pub async fn manifest() -> Response {
	(
		[
			(header::CONTENT_TYPE, HeaderValue::from_static("application/manifest+json")),
			(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
		],
		MANIFEST,
	)
		.into_response()
}

pub async fn icon(Path(name): Path<String>) -> Response {
	let bytes = match name.as_str() {
		"apple-touch-icon.png" => APPLE_TOUCH_ICON,
		"icon-512.png" => ICON_512,
		"icon-1024.png" => ICON_1024,
		_ => return StatusCode::NOT_FOUND.into_response(),
	};
	(
		[
			(header::CONTENT_TYPE, HeaderValue::from_static("image/png")),
			(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400")),
		],
		bytes,
	)
		.into_response()
}

pub async fn favicon() -> Response {
	icon(Path("apple-touch-icon.png".to_owned())).await
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn manifest_is_valid_json_and_icons_are_png() {
		let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
		assert_eq!(manifest["display"], "standalone");
		for icon in [APPLE_TOUCH_ICON, ICON_512, ICON_1024] {
			assert_eq!(&icon[1..4], b"PNG");
		}
	}
}
