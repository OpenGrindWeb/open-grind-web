//! Port of `src-tauri/src/error.rs`.
//!
//! The serialized shape (`{ "kind": …, "message": … }`) must stay identical:
//! the front parses it in `asAppError` (`src/lib/api/methods.ts`). The
//! `Recaptcha` and `Push` variants are left out until those features are
//! ported.

use std::fmt;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BanInfo {
	pub kind: String,
	pub code: i32,
	pub message: String,
	pub reason: Option<String>,
	pub sub_reason: Option<String>,
	pub automated: Option<bool>,
}

impl From<grindr::BanInfo> for BanInfo {
	fn from(b: grindr::BanInfo) -> Self {
		let kind = match b.kind {
			grindr::BanKind::Profile => "profile",
			grindr::BanKind::Device => "device",
			grindr::BanKind::Network => "network",
			grindr::BanKind::Underage => "underage",
			_ => "unknown",
		};
		Self {
			kind: kind.to_owned(),
			code: b.code,
			message: b.message,
			reason: b.reason,
			sub_reason: b.sub_reason,
			automated: b.automated,
		}
	}
}

/// Port of `src-tauri/src/api/recaptcha/error.rs` (only the variant the web
/// build can produce).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "reason", content = "detail")]
pub enum RecaptchaError {
	UnsupportedPlatform,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
	Recaptcha(RecaptchaError),
	Http(String),
	Connect(String),
	Auth(String),
	Media(String),
	NotSignedIn,
	SessionStale,
	Api { code: i32, message: String },
	Unauthorized { code: i32, message: String },
	Banned(BanInfo),
	RateLimited,
	RequestBlocked,
	NetworkBlocked,
	NotInitialized,
	SessionCleared,
	ContentTooLarge,
}

impl fmt::Display for AppError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			AppError::Http(msg) => write!(f, "HTTP error: {msg}"),
			AppError::Connect(msg) => write!(f, "Could not connect: {msg}"),
			AppError::Auth(msg) => write!(f, "Auth error: {msg}"),
			AppError::Media(msg) => write!(f, "Media error: {msg}"),
			AppError::NotSignedIn => write!(f, "Not signed in"),
			AppError::SessionStale => write!(f, "Could not refresh the session"),
			AppError::Api { code, message } => {
				write!(f, "API error {code}: {message}")
			}
			AppError::Unauthorized { code, message } => {
				write!(f, "Unauthorized ({code}): {message}")
			}
			AppError::Banned(info) => {
				write!(f, "Banned ({}): {}", info.kind, info.message)
			}
			AppError::RateLimited => write!(f, "Rate limited"),
			AppError::RequestBlocked => write!(f, "Request blocked by Cloudflare"),
			AppError::NetworkBlocked => {
				write!(f, "Request blocked before it reached Grindr")
			}
			AppError::SessionCleared => {
				write!(f, "Signed out while the request was in flight")
			}
			AppError::NotInitialized => write!(f, "GrindrClient not initialized"),
			AppError::ContentTooLarge => write!(f, "Larger than the upload limit"),
			AppError::Recaptcha(_) => write!(f, "reCAPTCHA error: unsupported platform"),
		}
	}
}

impl AppError {
	pub fn kind(&self) -> &'static str {
		match self {
			AppError::Http(_) => "Http",
			AppError::Connect(_) => "Connect",
			AppError::Auth(_) => "Auth",
			AppError::Media(_) => "Media",
			AppError::NotSignedIn => "NotSignedIn",
			AppError::SessionStale => "SessionStale",
			AppError::Api { .. } => "Api",
			AppError::Unauthorized { .. } => "Unauthorized",
			AppError::Banned(_) => "Banned",
			AppError::RateLimited => "RateLimited",
			AppError::RequestBlocked => "RequestBlocked",
			AppError::NetworkBlocked => "NetworkBlocked",
			AppError::NotInitialized => "NotInitialized",
			AppError::SessionCleared => "SessionCleared",
			AppError::ContentTooLarge => "ContentTooLarge",
			AppError::Recaptcha(_) => "Recaptcha",
		}
	}
}

impl std::error::Error for AppError {}

impl From<grindr::GrindrError> for AppError {
	fn from(e: grindr::GrindrError) -> Self {
		match e {
			grindr::GrindrError::Http(msg) => AppError::Http(msg),
			grindr::GrindrError::Connect(msg) => AppError::Connect(msg),
			grindr::GrindrError::Auth(msg) => AppError::Auth(msg),
			grindr::GrindrError::Api { code, message } => {
				AppError::Api { code, message }
			}
			grindr::GrindrError::Unauthorized { code, message } => {
				AppError::Unauthorized { code, message }
			}
			grindr::GrindrError::Banned(info) => AppError::Banned(info.into()),
			grindr::GrindrError::RateLimited => AppError::RateLimited,
			grindr::GrindrError::Blocked(grindr::BlockKind::Cloudflare) => {
				AppError::RequestBlocked
			}
			grindr::GrindrError::Blocked(_) => AppError::NetworkBlocked,
			grindr::GrindrError::SessionCleared => AppError::SessionCleared,
			_ => AppError::Http(e.to_string()),
		}
	}
}

enum SessionState {
	SignedOut,
	AwaitingFirstToken,
	Authorized,
}

fn session_state(client: &grindr::GrindrClient) -> SessionState {
	match client.session_receiver().borrow().as_ref() {
		None => SessionState::SignedOut,
		Some(session) if session.token.is_none() => {
			SessionState::AwaitingFirstToken
		}
		Some(_) => SessionState::Authorized,
	}
}

impl AppError {
	pub fn from_client_error(
		error: grindr::GrindrError,
		client: &grindr::GrindrClient,
	) -> Self {
		match (AppError::from(error), session_state(client)) {
			(AppError::Auth(_), SessionState::SignedOut) => AppError::NotSignedIn,
			(AppError::Auth(_), SessionState::AwaitingFirstToken) => {
				AppError::SessionStale
			}
			(mapped, _) => mapped,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn errors_serialize_as_kind_and_message() {
		let json = serde_json::to_value(AppError::Api {
			code: 404,
			message: "nope".into(),
		})
		.unwrap();
		assert_eq!(
			json,
			serde_json::json!({"kind": "Api", "message": {"code": 404, "message": "nope"}})
		);
		let json = serde_json::to_value(AppError::NotSignedIn).unwrap();
		assert_eq!(json, serde_json::json!({"kind": "NotSignedIn"}));
	}
}
