//! og-server: self-hosted web backend for Open Grind.
//!
//! Configuration (environment):
//! - `OG_PASSWORD_HASH` (required): argon2 PHC hash, from `og-server hash-password`
//! - `OG_BIND`: listen address, default `127.0.0.1:8080`
//! - `OG_STATIC_DIR`: SvelteKit build, default `./build`
//! - `OG_DATA_DIR`: device identity and session, default `./data`
//! - `OG_TRUST_PROXY=1`: take the client IP from `X-Forwarded-For` (behind Caddy)
//! - `OG_INSECURE_COOKIE=1`: drop the `Secure` cookie flag (local HTTP testing only)

mod auth;
mod error;
mod events;
mod gate;
mod ipc;
mod spa;
mod store;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::{header, HeaderValue};
use axum::routing::{get, post, MethodRouter};
use axum::Router;
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
}

impl AppState {
	pub fn client(&self) -> Result<grindr::GrindrClient, AppError> {
		self.client.clone().ok_or(AppError::NotInitialized)
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
		.route("/gate", get(gate::login_page))
		.route("/gate/login", post(gate::login))
		.route("/gate/logout", post(gate::logout))
		.route("/ipc/{command}", post(ipc::handler))
		.route("/events", get(events::handler))
		.fallback_service(assets)
		.layer(axum::middleware::from_fn_with_state(state.clone(), gate::require_session))
		.layer(CompressionLayer::new())
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

	if std::env::args().nth(1).as_deref() == Some("hash-password") {
		return hash_password();
	}

	let password_hash = std::env::var("OG_PASSWORD_HASH")
		.map_err(|_| "OG_PASSWORD_HASH is required (generate it with `og-server hash-password`)")?;
	let bind: SocketAddr = std::env::var("OG_BIND")
		.unwrap_or_else(|_| "127.0.0.1:8080".into())
		.parse()
		.map_err(|e| format!("OG_BIND: {e}"))?;
	let static_dir = PathBuf::from(std::env::var("OG_STATIC_DIR").unwrap_or_else(|_| "build".into()));
	let data_dir = PathBuf::from(std::env::var("OG_DATA_DIR").unwrap_or_else(|_| "data".into()));

	let gate = Gate::new(GateConfig {
		password_hash,
		secure_cookie: !env_flag("OG_INSECURE_COOKIE"),
		trust_proxy: env_flag("OG_TRUST_PROXY"),
	})?;

	let store = SecretStore::open(&data_dir).map_err(|e| format!("OG_DATA_DIR: {e}"))?;
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

	let state = AppState {
		client: Some(client),
		events,
		gate,
		index,
		app_data_dir,
		store,
		recovery,
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
