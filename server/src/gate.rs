//! Access gate in front of everything else.
//!
//! The server holds a live Grindr session, so nothing but the login page is
//! reachable without a valid `og_session` cookie. The password is checked
//! against an argon2 PHC hash (`og-server hash-password`), failures are
//! rate-limited per client IP, and session tokens are only kept as SHA-256
//! digests.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use argon2::password_hash::{PasswordHash, PasswordVerifier};
use argon2::Argon2;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::AppState;

pub const COOKIE: &str = "og_session";
const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(15 * 60);
/// Header the front shim sends on every IPC call. A cross-site HTML form
/// cannot set it, which backs up `SameSite=Strict`.
pub const IPC_HEADER: &str = "x-og-ipc";

const LOGIN_PAGE: &str = include_str!("gate.html");

pub struct GateConfig {
	pub password_hash: String,
	pub secure_cookie: bool,
	pub trust_proxy: bool,
}

#[derive(Default)]
struct Failures {
	count: u32,
	locked_until: Option<Instant>,
}

pub struct Gate {
	config: GateConfig,
	sessions: Mutex<HashMap<[u8; 32], Instant>>,
	failures: Mutex<HashMap<IpAddr, Failures>>,
}

impl Gate {
	pub fn new(config: GateConfig) -> Result<Arc<Self>, String> {
		PasswordHash::new(&config.password_hash)
			.map_err(|e| format!("OG_PASSWORD_HASH is not a valid PHC hash: {e}"))?;
		Ok(Arc::new(Self {
			config,
			sessions: Mutex::default(),
			failures: Mutex::default(),
		}))
	}

	fn digest(token: &str) -> [u8; 32] {
		Sha256::digest(token.as_bytes()).into()
	}

	fn is_valid(&self, token: &str) -> bool {
		let mut sessions = self.sessions.lock().unwrap();
		let now = Instant::now();
		sessions.retain(|_, expiry| *expiry > now);
		sessions.contains_key(&Self::digest(token))
	}

	fn open_session(&self) -> String {
		let mut bytes = [0u8; 32];
		getrandom::fill(&mut bytes).expect("OS random source");
		let token = URL_SAFE_NO_PAD.encode(bytes);
		self.sessions
			.lock()
			.unwrap()
			.insert(Self::digest(&token), Instant::now() + SESSION_TTL);
		token
	}

	fn close_session(&self, token: &str) {
		self.sessions.lock().unwrap().remove(&Self::digest(token));
	}

	fn locked(&self, ip: IpAddr) -> bool {
		let failures = self.failures.lock().unwrap();
		failures
			.get(&ip)
			.and_then(|f| f.locked_until)
			.is_some_and(|until| until > Instant::now())
	}

	fn record(&self, ip: IpAddr, success: bool) {
		let mut failures = self.failures.lock().unwrap();
		if success {
			failures.remove(&ip);
			return;
		}
		let entry = failures.entry(ip).or_default();
		if entry.locked_until.is_some_and(|until| until <= Instant::now()) {
			*entry = Failures::default();
		}
		entry.count += 1;
		if entry.count >= MAX_FAILURES {
			entry.locked_until = Some(Instant::now() + LOCKOUT);
			tracing::warn!("[gate] {ip} locked out after {} failed logins", entry.count);
		}
	}

	fn client_ip(&self, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
		if self.config.trust_proxy {
			// Right-most entry is the one appended by our own reverse proxy.
			if let Some(ip) = headers
				.get("x-forwarded-for")
				.and_then(|v| v.to_str().ok())
				.and_then(|v| v.rsplit(',').next())
				.and_then(|v| v.trim().parse().ok())
			{
				return ip;
			}
		}
		peer.ip()
	}

	fn cookie(&self, value: &str, max_age: u64) -> HeaderValue {
		let secure = if self.config.secure_cookie { "; Secure" } else { "" };
		HeaderValue::from_str(&format!(
			"{COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
		))
		.expect("cookie header")
	}
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
	headers
		.get_all(header::COOKIE)
		.iter()
		.filter_map(|v| v.to_str().ok())
		.flat_map(|v| v.split(';'))
		.filter_map(|pair| pair.trim().split_once('='))
		.find(|(name, _)| *name == COOKIE)
		.map(|(_, value)| value)
}

pub async fn login_page() -> Html<&'static str> {
	Html(LOGIN_PAGE)
}

#[derive(Deserialize)]
pub struct LoginForm {
	password: String,
}

pub async fn login(
	State(app): State<AppState>,
	ConnectInfo(peer): ConnectInfo<SocketAddr>,
	headers: HeaderMap,
	Form(form): Form<LoginForm>,
) -> Response {
	let gate = app.gate.clone();
	let ip = gate.client_ip(&headers, peer);
	if gate.locked(ip) {
		return Redirect::to("/gate?locked=1").into_response();
	}

	let hash = gate.config.password_hash.clone();
	let ok = tokio::task::spawn_blocking(move || {
		let parsed = PasswordHash::new(&hash).expect("validated at startup");
		Argon2::default()
			.verify_password(form.password.as_bytes(), &parsed)
			.is_ok()
	})
	.await
	.unwrap_or(false);

	gate.record(ip, ok);
	if !ok {
		tracing::info!("[gate] failed login from {ip}");
		return Redirect::to("/gate?failed=1").into_response();
	}

	let token = gate.open_session();
	let mut response = Redirect::to("/").into_response();
	response
		.headers_mut()
		.insert(header::SET_COOKIE, gate.cookie(&token, SESSION_TTL.as_secs()));
	response
}

pub async fn logout(State(app): State<AppState>, headers: HeaderMap) -> Response {
	if let Some(token) = session_cookie(&headers) {
		app.gate.close_session(token);
	}
	let mut response = Redirect::to("/gate").into_response();
	response
		.headers_mut()
		.insert(header::SET_COOKIE, app.gate.cookie("", 0));
	response
}

/// Middleware guarding every route except the gate itself.
pub async fn require_session(
	State(app): State<AppState>,
	request: Request,
	next: Next,
) -> Response {
	let path = request.uri().path();
	if path == "/gate" || path.starts_with("/gate/") {
		return next.run(request).await;
	}

	let authed = session_cookie(request.headers()).is_some_and(|t| app.gate.is_valid(t));
	let is_post_api = path.starts_with("/ipc/") || path == "/stage";
	let is_api = is_post_api || path == "/events" || path.starts_with("/media/");

	if !authed {
		return if is_api {
			(StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({"kind": "Gate"})))
				.into_response()
		} else {
			Redirect::to("/gate").into_response()
		};
	}

	if is_post_api
		&& (request.method() != Method::POST || !request.headers().contains_key(IPC_HEADER))
	{
		return StatusCode::FORBIDDEN.into_response();
	}

	next.run(request).await
}

#[cfg(test)]
mod tests {
	use super::*;

	fn gate() -> Arc<Gate> {
		use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
		let salt = SaltString::generate(&mut OsRng);
		let hash = Argon2::default()
			.hash_password(b"secret", &salt)
			.unwrap()
			.to_string();
		Gate::new(GateConfig {
			password_hash: hash,
			secure_cookie: true,
			trust_proxy: true,
		})
		.unwrap()
	}

	#[test]
	fn sessions_open_and_close() {
		let gate = gate();
		let token = gate.open_session();
		assert!(gate.is_valid(&token));
		assert!(!gate.is_valid("forged"));
		gate.close_session(&token);
		assert!(!gate.is_valid(&token));
	}

	#[test]
	fn repeated_failures_lock_the_ip() {
		let gate = gate();
		let ip: IpAddr = "203.0.113.7".parse().unwrap();
		for _ in 0..MAX_FAILURES - 1 {
			gate.record(ip, false);
		}
		assert!(!gate.locked(ip));
		gate.record(ip, false);
		assert!(gate.locked(ip));
		assert!(!gate.locked("203.0.113.8".parse().unwrap()));
	}

	#[test]
	fn forwarded_ip_is_the_right_most_entry_when_trusted() {
		let gate = gate();
		let mut headers = HeaderMap::new();
		headers.insert("x-forwarded-for", "1.1.1.1, 198.51.100.4".parse().unwrap());
		let peer: SocketAddr = "127.0.0.1:9".parse().unwrap();
		assert_eq!(
			gate.client_ip(&headers, peer),
			"198.51.100.4".parse::<IpAddr>().unwrap()
		);
	}

	#[test]
	fn reads_the_session_cookie_among_others() {
		let mut headers = HeaderMap::new();
		headers.insert(header::COOKIE, "a=1; og_session=abc; b=2".parse().unwrap());
		assert_eq!(session_cookie(&headers), Some("abc"));
	}
}
