//! Port of `src-tauri/src/api/auth.rs`, `session_recovery.rs` and the
//! command half of `ws.rs`.
//!
//! Email sign-in only: Google and Facebook depend on an embedded browser the
//! web build doesn't have, and reCAPTCHA minting is Android-only upstream too
//! (the desktop builds answer `UnsupportedPlatform` the same way).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::timeout;

use crate::error::{AppError, BanInfo, RecaptchaError};
use crate::events::EventHub;
use crate::store::{self, SecretStore};
use crate::AppState;

static SIGN_OUT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// --- shapes shared with the front --------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInResult {
	pub profile_id: String,
	pub restriction: Option<Restriction>,
}

impl From<grindr::SignInResult> for SignInResult {
	fn from(r: grindr::SignInResult) -> Self {
		Self {
			profile_id: r.profile_id,
			restriction: r.restriction.map(Restriction::from),
		}
	}
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restriction {
	pub kind: String,
	pub region: Option<String>,
	pub reason: Option<String>,
	pub sub_reason: Option<String>,
	pub ends_at: Option<i64>,
}

impl Restriction {
	fn of_kind(kind: &str) -> Self {
		Self {
			kind: kind.to_owned(),
			..Self::default()
		}
	}
}

impl From<grindr::Restriction> for Restriction {
	fn from(r: grindr::Restriction) -> Self {
		match r {
			grindr::Restriction::AgeVerification { region, reason } => Self {
				region: Some(region_str(region).to_owned()),
				reason: Some(reason),
				..Self::of_kind("ageVerification")
			},
			grindr::Restriction::TimedBan(details) => Self {
				reason: details.reason,
				sub_reason: details.sub_reason,
				ends_at: details.expiry_time,
				..Self::of_kind("timedBan")
			},
			grindr::Restriction::TrustVendorRejected => Self::of_kind("trustVendorRejected"),
			grindr::Restriction::Other(raw) => Self {
				reason: Some(raw),
				..Self::of_kind("other")
			},
			_ => Self::of_kind("other"),
		}
	}
}

fn region_str(region: grindr::VerificationRegion) -> &'static str {
	match region {
		grindr::VerificationRegion::Uk => "uk",
		grindr::VerificationRegion::Br => "br",
		grindr::VerificationRegion::Au => "au",
		_ => "other",
	}
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSession {
	pub profile_id: Option<u64>,
	pub expires_at: Option<u64>,
	pub stale: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionErrorPayload {
	pub message: String,
	pub unauthorized: bool,
	pub kind: String,
	pub attempts: u32,
	pub transient: bool,
}

#[derive(Debug, Serialize)]
pub struct RotateResult {
	#[serde(rename = "user-agent")]
	pub user_agent: String,
	#[serde(rename = "l-device-info")]
	pub l_device_info: String,
}

// --- arguments ---------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInWithEmailArgs {
	email: String,
	password: String,
	#[serde(default)]
	captcha_token: Option<String>,
}

#[derive(Deserialize)]
pub struct RefreshSessionArgs {
	#[serde(default)]
	geohash: Option<String>,
}

#[derive(Deserialize)]
pub struct SetAppActiveArgs {
	active: bool,
}

#[derive(Deserialize)]
pub struct WsSendArgs {
	command: grindr::WsCommand,
}

// --- session recovery state --------------------------------------------------

pub struct SessionRecovery {
	running: AtomicBool,
	pub foreground: AtomicBool,
}

impl Default for SessionRecovery {
	fn default() -> Self {
		Self {
			running: AtomicBool::new(false),
			foreground: AtomicBool::new(true),
		}
	}
}

const COALESCE: Duration = Duration::from_millis(1500);
const ATTEMPT_DELAYS: [Duration; 3] = [Duration::ZERO, Duration::from_secs(4), Duration::from_secs(12)];
const REFRESH_BUFFER_SECS: u64 = 60;
const DEPRECATED_CLIENT_VERSION: i32 = 8;
const RECONNECT_STEP_TIMEOUT: Duration = Duration::from_secs(3);

fn now_unix() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or(0)
}

fn session_of(session: Option<&grindr::Session>) -> CurrentSession {
	match session {
		Some(session) => {
			let expires_at = session.token.as_ref().map(|token| token.expires_at);
			CurrentSession {
				profile_id: session.credentials.profile_id.as_ref().and_then(|id| id.parse().ok()),
				expires_at,
				stale: expires_at.is_none_or(|at| at < now_unix() + REFRESH_BUFFER_SECS),
			}
		}
		None => CurrentSession::default(),
	}
}

fn feature_flags_of(session_id: &str) -> Vec<String> {
	#[derive(Deserialize)]
	#[serde(rename_all = "camelCase")]
	struct Claims {
		#[serde(default)]
		feature_flags: Vec<String>,
	}
	session_id
		.split('.')
		.nth(1)
		.and_then(|payload| URL_SAFE_NO_PAD.decode(payload).ok())
		.and_then(|json| serde_json::from_slice::<Claims>(&json).ok())
		.map(|claims| claims.feature_flags)
		.unwrap_or_default()
}

// --- commands ----------------------------------------------------------------

pub fn current_session(app: &AppState) -> CurrentSession {
	match app.client() {
		Ok(client) => session_of(client.session_receiver().borrow().as_ref()),
		Err(_) => CurrentSession::default(),
	}
}

pub fn account_restriction(app: &AppState) -> Option<Restriction> {
	let client = app.client().ok()?;
	let restriction = client
		.session_receiver()
		.borrow()
		.as_ref()
		.and_then(|s| s.token.as_ref()?.restriction.clone());
	restriction.map(Restriction::from)
}

pub fn session_feature_flags(app: &AppState) -> Vec<String> {
	let Ok(client) = app.client() else {
		return Vec::new();
	};
	let flags = client
		.session_receiver()
		.borrow()
		.as_ref()
		.and_then(|s| s.token.as_ref())
		.map(|token| feature_flags_of(&token.session_id))
		.unwrap_or_default();
	flags
}

pub async fn sign_in_with_email(app: &AppState, args: SignInWithEmailArgs) -> Result<SignInResult, AppError> {
	end_existing_session(app).await?;
	let client = app.client()?;
	let result = match args.captcha_token {
		Some(token) => client.sign_in_with_email_captcha(&args.email, &args.password, &token).await?,
		None => client.sign_in_with_email(&args.email, &args.password).await?,
	};
	Ok(SignInResult::from(result))
}

pub async fn refresh_session(app: &AppState, args: RefreshSessionArgs) -> Result<SignInResult, AppError> {
	let client = app.client()?;
	let result = client
		.refresh_session_at_geohash(args.geohash.as_deref())
		.await
		.map_err(|e| AppError::from_client_error(e, &client))?;
	Ok(SignInResult::from(result))
}

pub async fn sign_out(app: &AppState) -> Result<(), AppError> {
	end_session(&app.client()?, &app.store).await
}

async fn end_existing_session(app: &AppState) -> Result<(), AppError> {
	let client = app.client()?;
	if client.session_receiver().borrow().is_none() {
		return Ok(());
	}
	end_session(&client, &app.store).await
}

async fn end_session(client: &grindr::GrindrClient, store: &SecretStore) -> Result<(), AppError> {
	let _one_at_a_time = SIGN_OUT.lock().await;
	forget_account(client, store).await
}

/// Port of `forget_account`: drop the session and signing key, then move to a
/// fresh device identity so the next account doesn't share it.
async fn forget_account(client: &grindr::GrindrClient, store: &SecretStore) -> Result<(), AppError> {
	client.sign_out().await;
	store.delete(store::SESSION);
	store.delete(store::SIGNING_KEY);

	let new_device = grindr::DeviceInfo::generate();
	if let Err(e) = store.save(store::DEVICE_INFO, &new_device) {
		tracing::error!("[auth] could not persist rotated device info on sign out: {e}");
		store.delete(store::DEVICE_INFO);
	}
	client.rotate_device(new_device).await?;
	Ok(())
}

pub async fn rotate_api_params(app: &AppState) -> Result<RotateResult, AppError> {
	let client = app.client()?;
	let new_device = grindr::DeviceInfo::generate();
	if let Err(e) = app.store.save(store::DEVICE_INFO, &new_device) {
		tracing::error!("[client] could not persist rotated device info: {e}");
	}
	let old_device = client.rotate_device(new_device).await?;
	Ok(RotateResult {
		user_agent: grindr::build_user_agent(&old_device, "Free"),
		l_device_info: grindr::build_device_info_header(&old_device),
	})
}

pub async fn recaptcha_first_party_enabled(app: &AppState) -> Result<bool, AppError> {
	app.client()?.recaptcha_first_party_enabled().await.map_err(Into::into)
}

pub fn mint_recaptcha_token() -> AppError {
	AppError::Recaptcha(RecaptchaError::UnsupportedPlatform)
}

pub async fn set_app_active(app: &AppState, args: SetAppActiveArgs) -> Result<(), AppError> {
	app.recovery.foreground.store(args.active, Ordering::SeqCst);
	let client = app.client()?;
	let resuming = args.active && !client.is_active();
	client.set_active(args.active);
	if resuming {
		client.reset_transport().await?;
	}
	Ok(())
}

pub async fn ws_connect(app: &AppState) -> Result<(), AppError> {
	app.client()?.connect().await;
	Ok(())
}

pub async fn ws_reconnect(app: &AppState) -> Result<(), AppError> {
	let client = app.client()?;
	let mut states = client.connection_state();
	if *states.borrow_and_update() != grindr::WsConnectionState::Connected {
		return Ok(());
	}
	client.set_active(false);
	let stopped = timeout(
		RECONNECT_STEP_TIMEOUT,
		states.wait_for(|state| *state == grindr::WsConnectionState::Disconnected),
	)
	.await
	.is_ok();
	client.set_active(app.recovery.foreground.load(Ordering::SeqCst));
	if !stopped {
		return Err(AppError::Http("WS did not go down in time".to_owned()));
	}
	let _ = timeout(
		RECONNECT_STEP_TIMEOUT,
		states.wait_for(|state| *state == grindr::WsConnectionState::Connected),
	)
	.await;
	Ok(())
}

pub fn ws_send(app: &AppState, args: WsSendArgs) -> Result<(), AppError> {
	let client = app.client()?;
	// Same gate as upstream: a command sent while the socket is down must fail
	// instead of buffering until the next reconnect.
	if *client.connection_state().borrow() != grindr::WsConnectionState::Connected {
		return Err(AppError::Http("WS not connected".to_owned()));
	}
	client
		.ws_sender()
		.try_send(args.command)
		.map_err(|e| AppError::Http(format!("WS send failed: {e}")))
}

// --- background tasks --------------------------------------------------------

/// Port of the auth-event half of `spawn_ws_task` and of the restriction
/// watcher in the Tauri setup.
pub fn watch_auth(client: grindr::GrindrClient, store: SecretStore, hub: EventHub, recovery: std::sync::Arc<SessionRecovery>) {
	{
		let client = client.clone();
		let hub = hub.clone();
		let mut rx = client.auth_event_receiver();
		tokio::spawn(async move {
			loop {
				let event = match rx.recv().await {
					Ok(event) => event,
					Err(RecvError::Lagged(_)) => continue,
					Err(RecvError::Closed) => break,
				};
				match event {
					grindr::AuthEvent::SignedOut => {
						if let Err(e) = end_session(&client, &store).await {
							tracing::error!("[auth] could not rotate the device after the session ended: {e}");
						}
						hub.emit(
							"auth:session-error",
							SessionErrorPayload {
								message: "Session expired".to_owned(),
								unauthorized: true,
								kind: "Unauthorized".to_owned(),
								attempts: 0,
								transient: false,
							},
						);
					}
					grindr::AuthEvent::RefreshFailed { message, kind, .. } => {
						report_refresh_failure(&client, &hub, &recovery, message, kind.is_transient());
					}
					grindr::AuthEvent::RefreshRecovered => hub.emit("auth:session-ok", ()),
					grindr::AuthEvent::Banned(info) => {
						if let Err(e) = end_session(&client, &store).await {
							tracing::error!("[auth] could not rotate the device after a ban: {e}");
						}
						hub.emit("auth:banned", BanInfo::from(info));
					}
					_ => {}
				}
			}
		});
	}
	{
		let mut session_rx = client.session_receiver();
		tokio::spawn(async move {
			let mut restriction: Option<grindr::Restriction> = None;
			while session_rx.changed().await.is_ok() {
				let next = session_rx
					.borrow()
					.as_ref()
					.and_then(|session| session.token.as_ref()?.restriction.clone());
				if next != restriction {
					if let Some(r) = &next {
						hub.emit("auth:restriction", Restriction::from(r.clone()));
					}
					restriction = next;
				}
			}
		});
	}
}

fn report_refresh_failure(
	client: &grindr::GrindrClient,
	hub: &EventHub,
	recovery: &std::sync::Arc<SessionRecovery>,
	message: String,
	transient: bool,
) {
	if !transient {
		hub.emit(
			"auth:session-error",
			SessionErrorPayload {
				message,
				unauthorized: false,
				kind: "Auth".to_owned(),
				attempts: 0,
				transient: false,
			},
		);
		return;
	}
	if recovery
		.running
		.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
		.is_err()
	{
		return;
	}
	let client = client.clone();
	let hub = hub.clone();
	let recovery = recovery.clone();
	tokio::spawn(async move {
		let outcome = supervise(&client).await;
		recovery.running.store(false, Ordering::SeqCst);
		match outcome {
			Outcome::Quiet => {}
			Outcome::Failed(payload) => hub.emit("auth:session-error", payload),
			Outcome::Banned(info) => hub.emit("auth:banned", info),
			Outcome::UnsupportedVersion(error) => hub.emit("auth:unsupported-version", &error),
		}
	});
}

enum Outcome {
	Quiet,
	Failed(SessionErrorPayload),
	Banned(BanInfo),
	UnsupportedVersion(AppError),
}

fn settled_by(error: AppError, attempts: u32) -> Result<Outcome, AppError> {
	match error {
		AppError::Banned(info) => Ok(Outcome::Banned(info)),
		refused @ AppError::Api {
			code: DEPRECATED_CLIENT_VERSION,
			..
		} => Ok(Outcome::UnsupportedVersion(refused)),
		AppError::Unauthorized { .. } | AppError::NotSignedIn => Ok(Outcome::Failed(SessionErrorPayload {
			message: error.to_string(),
			unauthorized: true,
			kind: error.kind().to_owned(),
			attempts,
			transient: false,
		})),
		other => Err(other),
	}
}

async fn supervise(client: &grindr::GrindrClient) -> Outcome {
	tokio::time::sleep(COALESCE).await;
	let mut attempts = 0;
	let mut last: Option<AppError> = None;
	for delay in ATTEMPT_DELAYS {
		tokio::time::sleep(delay).await;
		if !client.is_active() || !session_of(client.session_receiver().borrow().as_ref()).stale {
			return Outcome::Quiet;
		}
		attempts += 1;
		match client.refresh_session().await {
			Ok(_) => return Outcome::Quiet,
			Err(error) => match settled_by(AppError::from_client_error(error, client), attempts) {
				Ok(outcome) => return outcome,
				Err(retryable) => last = Some(retryable),
			},
		}
	}
	match last {
		Some(error) => Outcome::Failed(SessionErrorPayload {
			message: error.to_string(),
			unauthorized: false,
			kind: error.kind().to_owned(),
			attempts,
			transient: true,
		}),
		None => Outcome::Quiet,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn session(expires_at: Option<u64>) -> grindr::Session {
		grindr::Session {
			credentials: grindr::Credentials {
				email: "a@b.c".to_owned(),
				profile_id: Some("42".to_owned()),
				auth_token: "tok".to_owned(),
				kind: grindr::SessionKind::Email,
				third_party_user_id: None,
			},
			token: expires_at.map(|expires_at| grindr::SessionToken {
				session_id: "sid".to_owned(),
				expires_at,
				restriction: None,
			}),
		}
	}

	#[test]
	fn current_session_shapes() {
		assert!(serde_json::to_value(session_of(None)).unwrap() == serde_json::json!({"profileId": null, "expiresAt": null, "stale": false}));
		let fresh = session_of(Some(&session(Some(now_unix() + 3600))));
		assert_eq!(fresh.profile_id, Some(42));
		assert!(!fresh.stale);
		assert!(session_of(Some(&session(Some(now_unix())))).stale);
		assert!(session_of(Some(&session(None))).stale);
	}

	#[test]
	fn feature_flags_come_from_the_jwt_payload() {
		let payload = URL_SAFE_NO_PAD.encode(br#"{"featureFlags":["a","b"]}"#);
		assert_eq!(feature_flags_of(&format!("h.{payload}.s")), vec!["a", "b"]);
		assert!(feature_flags_of("garbage").is_empty());
	}

	#[test]
	fn sign_in_args_accept_the_camel_case_captcha_token() {
		let args: SignInWithEmailArgs =
			serde_json::from_value(serde_json::json!({"email": "e", "password": "p", "captchaToken": "t"})).unwrap();
		assert_eq!(args.captcha_token.as_deref(), Some("t"));
	}

	#[test]
	fn recaptcha_is_reported_as_unsupported() {
		assert_eq!(
			serde_json::to_value(mint_recaptcha_token()).unwrap(),
			serde_json::json!({"kind": "Recaptcha", "message": {"reason": "unsupportedPlatform"}})
		);
	}
}
