//! og-server: self-hosted web backend for Open Grind.
//!
//! Configuration (environment):
//! - `OG_PASSWORD_HASH`: argon2 PHC hash, from `og-server hash-password`
//! - `OG_TOTP_SECRET`: base32 authenticator secret, from `og-server totp-setup`;
//!   lets a 6-digit code open the gate instead of the password
//!   (at least one of the two is required)
//! - `OG_BIND`: listen address, default `127.0.0.1:8080`
//! - `OG_STATIC_DIR`: SvelteKit build, default `./build`
//! - `OG_DATA_DIR`: device identity and session, default `./data`
//! - `OG_TRUST_PROXY=<n>`: n reverse proxies in front (usually 1); the client
//!   IP for the lockout comes from `X-Forwarded-For`, trusted only when the
//!   request arrives from a loopback or private address
//! - `OG_INSECURE_COOKIE=1`: drop the `Secure` cookie flag (local HTTP testing only)

mod auth;
mod error;
mod events;
mod gate;
mod ipc;
mod media;
mod photo;
mod pwa;
mod spa;
mod store;
mod totp;
mod upload;

// Compiled unchanged from the upstream Tauri tree.
#[path = "../../src-tauri/src/hex.rs"]
mod hex;
#[path = "../../src-tauri/src/video/mod.rs"]
mod video;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::{header, HeaderValue};
use axum::routing::{get, post, MethodRouter};
use axum::Router;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::error::AppError;
use crate::events::EventHub;
use crate::gate::{Gate, GateConfig};
use crate::store::SecretStore;

#[derive(Clone)]
pub struct AppState {
	client: Option<grindr::GrindrClient>,
	pub events: EventHub,
	pub gate: Arc<Gate>,
	pub index: spa::Index,
	pub app_data_dir: PathBuf,
	pub store: SecretStore,
	pub recovery: Arc<auth::SessionRecovery>,
	pub staging: upload::Staging,
}

impl AppState {
	pub fn client(&self) -> Result<grindr::GrindrClient, AppError> {
		self.client.clone().ok_or(AppError::NotInitialized)
	}
}

/// `OG_TRUST_PROXY`: how many reverse proxies sit in front (a number;
/// `true`/`yes` mean 1; unset or 0 means none).
fn trusted_proxies() -> Result<usize, String> {
	match std::env::var("OG_TRUST_PROXY").as_deref() {
		Err(_) | Ok("") | Ok("0") | Ok("false") | Ok("no") => Ok(0),
		Ok("true" | "yes") => Ok(1),
		Ok(n) => n
			.parse()
			.map_err(|_| format!("OG_TRUST_PROXY must be a number of proxies, got {n:?}")),
	}
}

fn env_flag(name: &str) -> bool {
	matches!(std::env::var(name).as_deref(), Ok("1" | "true" | "yes"))
}

fn hash_password() -> Result<(), String> {
	use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
	let mut line = String::new();
	eprintln!("Password (read from stdin):");
	std::io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
	let password = line.trim_end_matches(['\r', '\n']);
	if password.chars().count() < 12 {
		return Err("use at least 12 characters".into());
	}
	let salt = SaltString::generate(&mut OsRng);
	let hash = argon2::Argon2::default()
		.hash_password(password.as_bytes(), &salt)
		.map_err(|e| e.to_string())?;
	println!("{hash}");
	Ok(())
}

/// Prints a new TOTP secret with its QR code, to scan in an authenticator
/// app (iOS Passwords, Google Authenticator, ...), then set OG_TOTP_SECRET.
fn totp_setup() -> Result<(), String> {
	let secret = totp::Totp::generate_secret();
	let uri = totp::Totp::provisioning_uri(&secret, "server");
	println!("{}", totp::qr_for_terminal(&uri)?);
	println!("Scan the QR code with your authenticator app, or enter this key:");
	println!();
	println!("    {secret}");
	println!();
	println!("Then start og-server with:");
	println!();
	println!("    OG_TOTP_SECRET={secret}");
	println!();
	println!("Keep it secret: anyone with it can open the gate.");
	Ok(())
}

/// Port of the session persistence loop in `src-tauri/src/lib.rs` setup.
fn persist_session(client: &grindr::GrindrClient, store: SecretStore, mut stored: Option<grindr::Credentials>) {
	let mut session_rx = client.session_receiver();
	tokio::spawn(async move {
		while session_rx.changed().await.is_ok() {
			let next = session_rx
				.borrow()
				.as_ref()
				.map(|session| session.credentials.clone());
			if next == stored {
				continue;
			}
			match &next {
				Some(c) => {
					if let Err(e) = store.save(store::SESSION, c) {
						tracing::error!("[session] persist failed: {e}");
					}
				}
				None => store.delete(store::SESSION),
			}
			stored = next;
		}
	});
}

/// Port of the signing key loop in the Tauri setup: restore the device key
/// used by signed uploads, then persist every change.
fn persist_signing_key(client: &grindr::GrindrClient, store: SecretStore) {
	let client = client.clone();
	let mut key_rx = client.signing_key_receiver();
	tokio::spawn(async move {
		if let Ok(Some(key)) = store.load::<grindr::DeviceSigningKey>(store::SIGNING_KEY) {
			if !client.restore_signing_key(key).await {
				tracing::warn!("[signing] stored key was refused, deleting it");
				store.delete(store::SIGNING_KEY);
			}
		}
		while key_rx.changed().await.is_ok() {
			let key = key_rx.borrow().clone();
			match key {
				Some(k) => {
					if let Err(e) = store.save(store::SIGNING_KEY, &k) {
						tracing::error!("[signing] persist failed: {e}");
					}
				}
				None => store.delete(store::SIGNING_KEY),
			}
		}
	});
}

pub fn router(state: AppState, static_dir: PathBuf) -> Router {
	fn spa_index<S: Clone + Send + Sync + 'static>(index: &spa::Index) -> MethodRouter<S> {
		let index = index.clone();
		get(move || {
			let index = index.clone();
			async move { index.response() }
		})
	}
	let assets = ServeDir::new(&static_dir)
		.append_index_html_on_directories(false)
		.fallback(spa_index::<()>(&state.index));

	Router::new()
		.route("/", spa_index(&state.index))
		.route("/index.html", spa_index(&state.index))
		.route("/og-bridge.js", get(spa::bridge))
		.route("/manifest.webmanifest", get(pwa::manifest))
		.route("/og-icons/{name}", get(pwa::icon))
		.route("/favicon.ico", get(pwa::favicon))
		.route("/gate", get(gate::login_page))
		.route("/gate/login", post(gate::login))
		.route("/gate/logout", post(gate::logout))
		.route("/ipc/{command}", post(ipc::handler))
		.route("/events", get(events::handler))
		.route("/media/{scheme}/{path}", get(media::handler))
		.route(
			"/stage",
			post(upload::stage).layer(axum::extract::DefaultBodyLimit::max(upload::MAX_STAGED_BYTES)),
		)
		.fallback_service(assets)
		.layer(axum::middleware::from_fn_with_state(state.clone(), gate::require_session))
		// Images are already skipped by the default predicate; video must be
		// too, or range requests would get a compressed body.
		.layer(CompressionLayer::new().compress_when(
			DefaultPredicate::new().and(NotForContentType::const_new("video/")),
		))
		.layer(SetResponseHeaderLayer::overriding(
			header::X_FRAME_OPTIONS,
			HeaderValue::from_static("DENY"),
		))
		.layer(SetResponseHeaderLayer::overriding(
			header::X_CONTENT_TYPE_OPTIONS,
			HeaderValue::from_static("nosniff"),
		))
		.layer(SetResponseHeaderLayer::overriding(
			header::REFERRER_POLICY,
			HeaderValue::from_static("no-referrer"),
		))
		.layer(SetResponseHeaderLayer::overriding(
			header::HeaderName::from_static("x-robots-tag"),
			HeaderValue::from_static("noindex, nofollow"),
		))
		.with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), String> {
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| "info".into()),
		)
		.init();

	match std::env::args().nth(1).as_deref() {
		Some("hash-password") => return hash_password(),
		Some("totp-setup") => return totp_setup(),
		_ => {}
	}

	let password_hash = std::env::var("OG_PASSWORD_HASH").ok().filter(|v| !v.is_empty());
	let totp = match std::env::var("OG_TOTP_SECRET").ok().filter(|v| !v.is_empty()) {
		Some(secret) => Some(totp::Totp::from_base32(&secret)?),
		None => None,
	};
	let bind: SocketAddr = std::env::var("OG_BIND")
		.unwrap_or_else(|_| "127.0.0.1:8080".into())
		.parse()
		.map_err(|e| format!("OG_BIND: {e}"))?;
	let static_dir = PathBuf::from(std::env::var("OG_STATIC_DIR").unwrap_or_else(|_| "build".into()));
	let data_dir = PathBuf::from(std::env::var("OG_DATA_DIR").unwrap_or_else(|_| "data".into()));

	let store = SecretStore::open(&data_dir).map_err(|e| format!("OG_DATA_DIR: {e}"))?;
	let gate = Gate::new(
		GateConfig {
			password_hash,
			totp,
			secure_cookie: !env_flag("OG_INSECURE_COOKIE"),
			trusted_proxies: trusted_proxies()?,
		},
		Some(data_dir.join("secrets").join("gate-sessions.msgpack")),
	)?;
	let device = store.device_or_create();
	let credentials = store
		.load::<grindr::Credentials>(store::SESSION)
		.unwrap_or_else(|e| {
			tracing::warn!("[setup] could not load credentials: {e}");
			None
		});
	let resumed = credentials.clone().map(|credentials| grindr::Session {
		credentials,
		token: None,
	});
	let client = grindr::GrindrClient::new(device, resumed)
		.map_err(|e| format!("failed to build GrindrClient: {e}"))?;
	persist_session(&client, store.clone(), credentials);
	persist_signing_key(&client, store.clone());

	let events = EventHub::new();
	events::forward_client_events(client.clone(), events.clone());
	let recovery = Arc::new(auth::SessionRecovery::default());
	auth::watch_auth(client.clone(), store.clone(), events.clone(), recovery.clone());

	let index = spa::Index::load(&static_dir);
	if !static_dir.join("index.html").exists() {
		tracing::warn!("[setup] {} has no index.html: build the front first", static_dir.display());
	}
	let app_data_dir = data_dir.join("app-data");
	std::fs::create_dir_all(&app_data_dir).map_err(|e| format!("OG_DATA_DIR: {e}"))?;
	let staging = upload::Staging::open(&data_dir).map_err(|e| format!("OG_DATA_DIR: {e}"))?;
	staging.spawn_janitor();

	let state = AppState {
		client: Some(client),
		events,
		gate,
		index,
		app_data_dir,
		store,
		recovery,
		staging,
	};

	let listener = tokio::net::TcpListener::bind(bind)
		.await
		.map_err(|e| format!("bind {bind}: {e}"))?;
	tracing::info!("og-server listening on {bind}");
	axum::serve(
		listener,
		router(state, static_dir).into_make_service_with_connect_info::<SocketAddr>(),
	)
	.with_graceful_shutdown(async {
		let _ = tokio::signal::ctrl_c().await;
	})
	.await
	.map_err(|e| e.to_string())
}
