//! Access gate in front of everything else.
//!
//! The server holds a live Grindr session, so nothing but the login page is
//! reachable without a valid `og_session` cookie. The password is checked
//! against an argon2 PHC hash (`og-server hash-password`), failures are
//! rate-limited per client IP, and session tokens are only kept as SHA-256
//! digests.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

use crate::totp::{Totp, Verdict};
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
	/// argon2 PHC hash; `None` when only codes are allowed.
	pub password_hash: Option<String>,
	/// Authenticator codes (TOTP), accepted instead of the password.
	pub totp: Option<Totp>,
	pub secure_cookie: bool,
	/// Reverse proxies in front of the server (0: none). With n, the client
	/// is the n-th `X-Forwarded-For` entry from the right.
	pub trusted_proxies: usize,
}

#[derive(Default)]
struct Failures {
	count: u32,
	locked_until: Option<Instant>,
}

type Sessions = HashMap<[u8; 32], u64>;

pub struct Gate {
	config: GateConfig,
	/// SHA-256 of each session token -> expiry (unix seconds).
	sessions: Mutex<Sessions>,
	/// Where sessions survive restarts (`None` in tests).
	sessions_file: Option<PathBuf>,
	failures: Mutex<HashMap<IpAddr, Failures>>,
}

fn now_unix() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or(0)
}

impl Gate {
	pub fn new(config: GateConfig, sessions_file: Option<PathBuf>) -> Result<Arc<Self>, String> {
		if let Some(hash) = &config.password_hash {
			PasswordHash::new(hash)
				.map_err(|e| format!("OG_PASSWORD_HASH is not a valid PHC hash: {e}"))?;
		}
		if config.password_hash.is_none() && config.totp.is_none() {
			return Err("set OG_PASSWORD_HASH, OG_TOTP_SECRET or both".into());
		}
		let sessions = sessions_file.as_deref().map(load_sessions).unwrap_or_default();
		Ok(Arc::new(Self {
			config,
			sessions: Mutex::new(sessions),
			sessions_file,
			failures: Mutex::default(),
		}))
	}

	fn digest(token: &str) -> [u8; 32] {
		Sha256::digest(token.as_bytes()).into()
	}

	fn is_valid(&self, token: &str) -> bool {
		let mut sessions = self.sessions.lock().unwrap();
		let now = now_unix();
		sessions.retain(|_, expiry| *expiry > now);
		sessions.contains_key(&Self::digest(token))
	}

	fn open_session(&self) -> String {
		let mut bytes = [0u8; 32];
		getrandom::fill(&mut bytes).expect("OS random source");
		let token = URL_SAFE_NO_PAD.encode(bytes);
		let mut sessions = self.sessions.lock().unwrap();
		sessions.insert(Self::digest(&token), now_unix() + SESSION_TTL.as_secs());
		self.persist(&sessions);
		token
	}

	fn close_session(&self, token: &str) {
		let mut sessions = self.sessions.lock().unwrap();
		sessions.remove(&Self::digest(token));
		self.persist(&sessions);
	}

	/// Only digests are written, never the tokens themselves: reading the
	/// file doesn't let anyone in.
	fn persist(&self, sessions: &Sessions) {
		let Some(path) = &self.sessions_file else { return };
		let now = now_unix();
		let live: Vec<([u8; 32], u64)> = sessions
			.iter()
			.filter(|(_, expiry)| **expiry > now)
			.map(|(digest, expiry)| (*digest, *expiry))
			.collect();
		let result = rmp_serde::encode::to_vec(&live)
			.map_err(|e| e.to_string())
			.and_then(|bytes| {
				let tmp = path.with_extension("tmp");
				std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
				restrict(&tmp);
				std::fs::rename(&tmp, path).map_err(|e| e.to_string())
			});
		if let Err(e) = result {
			tracing::error!("[gate] could not persist sessions: {e}");
		}
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

	/// The client's IP, for the lockout. Behind trusted proxies it comes from
	/// `X-Forwarded-For`, but only when the request really arrives from a
	/// proxy on this machine or a private network: a client reaching the port
	/// directly could otherwise send a fresh made-up IP with every guess.
	fn client_ip(&self, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
		let hops = self.config.trusted_proxies;
		if hops > 0 && is_local(peer.ip()) {
			let forwarded: Vec<&str> = headers
				.get_all("x-forwarded-for")
				.iter()
				.filter_map(|v| v.to_str().ok())
				.flat_map(|v| v.split(','))
				.map(str::trim)
				.filter(|v| !v.is_empty())
				.collect();
			// Each trusted proxy appends the address it received from, so
			// the client is `hops` entries from the right.
			if let Some(ip) = forwarded
				.len()
				.checked_sub(hops)
				.and_then(|index| forwarded[index].parse().ok())
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

/// Loopback, private (RFC 1918 / unique local) or link-local: where a
/// reverse proxy of ours can be.
fn is_local(ip: IpAddr) -> bool {
	match ip {
		IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
		IpAddr::V6(v6) => {
			if let Some(v4) = v6.to_ipv4_mapped() {
				return is_local(IpAddr::V4(v4));
			}
			v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80
		}
	}
}

fn load_sessions(path: &Path) -> Sessions {
	let Ok(bytes) = std::fs::read(path) else {
		return Sessions::new();
	};
	let now = now_unix();
	rmp_serde::from_slice::<Vec<([u8; 32], u64)>>(&bytes)
		.map(|live| live.into_iter().filter(|(_, expiry)| *expiry > now).collect())
		.unwrap_or_else(|e| {
			tracing::warn!("[gate] ignoring unreadable sessions file: {e}");
			Sessions::new()
		})
}

#[cfg(unix)]
fn restrict(path: &Path) {
	use std::os::unix::fs::PermissionsExt;
	let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

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

pub async fn login_page(State(app): State<AppState>) -> Html<String> {
	let config = &app.gate.config;
	let modes = match (config.totp.is_some(), config.password_hash.is_some()) {
		(true, true) => "both",
		(true, false) => "code",
		_ => "password",
	};
	Html(LOGIN_PAGE.replace("{{MODES}}", modes))
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

	// A 6-digit entry is an authenticator code when codes are enabled;
	// anything else is the password.
	let ok = match &gate.config.totp {
		Some(totp) if Totp::looks_like_code(&form.password) => match totp.verify(&form.password) {
			Verdict::Accepted => true,
			Verdict::Rejected => false,
			Verdict::Paused => {
				tracing::warn!("[gate] code logins paused: too many failed codes this hour");
				return Redirect::to("/gate?paused=1").into_response();
			}
		},
		_ => match gate.config.password_hash.clone() {
			Some(hash) => tokio::task::spawn_blocking(move || {
				let parsed = PasswordHash::new(&hash).expect("validated at startup");
				Argon2::default()
					.verify_password(form.password.as_bytes(), &parsed)
					.is_ok()
			})
			.await
			.unwrap_or(false),
			None => false,
		},
	};

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
	if path == "/gate" || path.starts_with("/gate/") || crate::pwa::is_public(path) {
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
		Gate::new(
			GateConfig {
				password_hash: Some(hash),
				totp: None,
				secure_cookie: true,
				trusted_proxies: 1,
			},
			None,
		)
		.unwrap()
	}

	#[test]
	fn sessions_survive_a_restart_and_only_digests_hit_the_disk() {
		let file = std::env::temp_dir().join(format!("og-gate-{}.msgpack", std::process::id()));
		let hash = gate().config.password_hash.clone();
		let config = || GateConfig {
			password_hash: hash.clone(),
			totp: None,
			secure_cookie: true,
			trusted_proxies: 0,
		};
		let first = Gate::new(config(), Some(file.clone())).unwrap();
		let token = first.open_session();
		let on_disk = std::fs::read(&file).unwrap();
		assert!(!on_disk.windows(token.len()).any(|w| w == token.as_bytes()));

		let restarted = Gate::new(config(), Some(file.clone())).unwrap();
		assert!(restarted.is_valid(&token));
		restarted.close_session(&token);
		assert!(!Gate::new(config(), Some(file.clone())).unwrap().is_valid(&token));
		let _ = std::fs::remove_file(file);
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
	fn forwarded_ip_counts_proxy_hops_and_needs_a_local_peer() {
		let mut gate = Arc::into_inner(gate()).unwrap();
		gate.config.trusted_proxies = 2;
		let mut headers = HeaderMap::new();
		headers.insert("x-forwarded-for", "6.6.6.6, 198.51.100.4, 10.0.0.2".parse().unwrap());
		let lan: SocketAddr = "192.168.1.20:443".parse().unwrap();
		assert_eq!(gate.client_ip(&headers, lan), "198.51.100.4".parse::<IpAddr>().unwrap());

		// straight from the internet: the header is the client's own claim
		let public: SocketAddr = "203.0.113.9:5000".parse().unwrap();
		assert_eq!(gate.client_ip(&headers, public), public.ip());

		// fewer entries than proxies: fall back to the peer
		let mut short = HeaderMap::new();
		short.insert("x-forwarded-for", "10.0.0.2".parse().unwrap());
		assert_eq!(gate.client_ip(&short, lan), lan.ip());
	}

	#[test]
	fn reads_the_session_cookie_among_others() {
		let mut headers = HeaderMap::new();
		headers.insert(header::COOKIE, "a=1; og_session=abc; b=2".parse().unwrap());
		assert_eq!(session_cookie(&headers), Some("abc"));
	}
}
