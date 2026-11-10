//! HTTP replacement for Tauri's `invoke`.
//!
//! `POST /ipc/<command>` with the same JSON arguments the front passes to
//! `invoke(command, args)`. Success: `200` + the command's JSON result.
//! Failure: `422` + the serialized `AppError`, which the shim rethrows so the
//! existing error handling (`asAppError`) keeps working unchanged.
//!
//! Only commands listed in `dispatch` are reachable.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::error::AppError;
use crate::AppState;

type IpcResult = Result<serde_json::Value, AppError>;

pub async fn handler(
	State(app): State<AppState>,
	Path(command): Path<String>,
	body: Option<Json<serde_json::Value>>,
) -> Response {
	let args = body.map(|Json(v)| v).unwrap_or(serde_json::Value::Null);
	match dispatch(&app, &command, args).await {
		Some(Ok(value)) => Json(value).into_response(),
		Some(Err(error)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(error)).into_response(),
		None => {
			tracing::debug!("[ipc] unknown or unported command: {command}");
			(
				StatusCode::NOT_FOUND,
				Json(AppError::Http(format!("command not available on the web build: {command}"))),
			)
				.into_response()
		}
	}
}

async fn dispatch(app: &AppState, command: &str, args: serde_json::Value) -> Option<IpcResult> {
	Some(match command {
		"backend_ready" => to_json(app.client().is_ok()),
		"request" => match parse::<RequestArgs>(args) {
			Ok(a) => request(app, a).await.map(serde_json::Value::String),
			Err(e) => Err(e),
		},
		"fetch_external" => match parse::<FetchExternalArgs>(args) {
			Ok(a) => fetch_external(app, a).await.map(serde_json::Value::String),
			Err(e) => Err(e),
		},
		"read_app_data" | "write_app_data" | "remove_app_data" => {
			match parse::<AppDataArgs>(args) {
				Ok(a) => app_data(app, command, a).await,
				Err(e) => Err(e),
			}
		}
		_ => return None,
	})
}

fn parse<T: DeserializeOwned>(args: serde_json::Value) -> Result<T, AppError> {
	serde_json::from_value(args).map_err(|e| AppError::Http(format!("invalid arguments: {e}")))
}

fn to_json(value: impl Serialize) -> IpcResult {
	serde_json::to_value(value).map_err(|e| AppError::Http(e.to_string()))
}

// --- Port of src-tauri/src/api/rest.rs -------------------------------------

const SIGNED_UPLOAD_PATHS: [&str; 2] = ["/v5/media/upload", "/v6/chat/media/upload"];
const MAX_EXTERNAL_BYTES: usize = 4 * 1024 * 1024;

fn requires_device_signature(path: &str) -> bool {
	let path = path.split(['?', '#']).next().unwrap_or(path);
	SIGNED_UPLOAD_PATHS.contains(&path)
}

fn refuse_signed_path(path: &str) -> Result<(), AppError> {
	if requires_device_signature(path) {
		return Err(AppError::Api {
			code: 400,
			message: format!("{path} needs the signed upload command"),
		});
	}
	Ok(())
}

#[derive(Serialize, Deserialize)]
struct RawResponse {
	status: u16,
	#[serde(with = "serde_bytes")]
	body: Vec<u8>,
}

#[derive(Deserialize)]
struct RequestPayload {
	method: String,
	path: String,
	#[serde(with = "serde_bytes")]
	#[serde(default)]
	body: Option<Vec<u8>>,
}

#[derive(Deserialize)]
struct RequestArgs {
	payload: String,
}

#[derive(Deserialize)]
struct FetchExternalArgs {
	url: String,
	range: Option<String>,
}

fn decode_request(payload: &str) -> Result<RequestPayload, AppError> {
	let bytes = STANDARD
		.decode(payload)
		.map_err(|e| AppError::Http(format!("Failed to decode base64 payload: {e}")))?;
	rmp_serde::from_slice(&bytes)
		.map_err(|e| AppError::Http(format!("Failed to decode request payload: {e}")))
}

fn encode_response(response: &RawResponse) -> Result<String, AppError> {
	rmp_serde::encode::to_vec_named(response)
		.map(|bytes| STANDARD.encode(&bytes))
		.map_err(|e| AppError::Http(e.to_string()))
}

async fn request(app: &AppState, args: RequestArgs) -> Result<String, AppError> {
	let payload = decode_request(&args.payload)?;
	refuse_signed_path(&payload.path)?;
	let method = grindr::Method::from_str(&payload.method).map_err(|_| AppError::Api {
		code: 400,
		message: format!("Invalid method: {}", payload.method),
	})?;

	let json_body: Option<serde_json::Value> = match payload.body {
		Some(b) => Some(
			rmp_serde::from_slice(&b)
				.map_err(|e| AppError::Http(format!("Failed to decode msgpack body: {e}")))?,
		),
		None => None,
	};

	let client = app.client()?;
	let request = client.request(method, &payload.path);
	let request = match &json_body {
		Some(body) => request.json(body),
		None => request,
	};
	let raw = request
		.send()
		.await
		.map_err(|e| AppError::from_client_error(e, &client))?;

	encode_response(&RawResponse {
		status: raw.status,
		body: raw.body,
	})
}

async fn fetch_external(app: &AppState, args: FetchExternalArgs) -> Result<String, AppError> {
	let client = app.client()?;
	let response = client
		.fetch_media(grindr::MediaRequest {
			url: &args.url,
			range: args.range.as_deref(),
			max_bytes: MAX_EXTERNAL_BYTES,
			fetcher: grindr::MediaFetcher::Plain,
		})
		.await
		.map_err(|e| AppError::from_client_error(e, &client))?;

	encode_response(&RawResponse {
		status: response.status,
		body: Vec::from(response.body),
	})
}

// --- Port of src-tauri/src/app_data.rs ---------------------------------------

/// Same closed set of files as `AppDataFile` upstream; the name never reaches
/// the filesystem unvalidated.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum AppDataFile {
	Preferences,
	HondurasHold,
}

impl AppDataFile {
	fn file_name(self) -> &'static str {
		match self {
			AppDataFile::Preferences => "preferences.data",
			AppDataFile::HondurasHold => "honduras-hold.data",
		}
	}
}

#[derive(Deserialize)]
struct AppDataArgs {
	file: AppDataFile,
	#[serde(default)]
	content: Option<String>,
}

async fn app_data(app: &AppState, command: &str, args: AppDataArgs) -> IpcResult {
	let path = app.app_data_dir.join(args.file.file_name());
	let io = |e: std::io::Error| AppError::Http(format!("app data: {e}"));
	match command {
		"read_app_data" => match tokio::fs::read(&path).await {
			Ok(bytes) => to_json(bytes),
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Value::Null),
			Err(e) => Err(io(e)),
		},
		"write_app_data" => {
			let bytes = STANDARD
				.decode(args.content.unwrap_or_default())
				.map_err(|e| AppError::Http(format!("app data: {e}")))?;
			let tmp = path.with_extension("tmp");
			tokio::fs::write(&tmp, &bytes).await.map_err(io)?;
			tokio::fs::rename(&tmp, &path).await.map_err(io)?;
			Ok(serde_json::Value::Null)
		}
		_ => match tokio::fs::remove_file(&path).await {
			Ok(()) => Ok(serde_json::Value::Null),
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Value::Null),
			Err(e) => Err(io(e)),
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn app_data_names_are_a_closed_set() {
		let ok: AppDataArgs = serde_json::from_value(serde_json::json!({"file": "hondurasHold"})).unwrap();
		assert_eq!(ok.file.file_name(), "honduras-hold.data");
		assert!(serde_json::from_value::<AppDataArgs>(serde_json::json!({"file": "../../etc/passwd"})).is_err());
	}

	#[derive(Serialize)]
	struct Wire<'a> {
		method: &'a str,
		path: &'a str,
		#[serde(with = "serde_bytes")]
		body: Option<Vec<u8>>,
	}

	#[test]
	fn decodes_the_payload_the_front_sends() {
		let body = serde_json::json!({ "targetProfileIds": [1, 2] });
		let payload = STANDARD.encode(
			rmp_serde::encode::to_vec_named(&Wire {
				method: "POST",
				path: "/v3/profiles",
				body: Some(rmp_serde::encode::to_vec_named(&body).unwrap()),
			})
			.unwrap(),
		);
		let decoded = decode_request(&payload).unwrap();
		assert_eq!(decoded.method, "POST");
		assert_eq!(decoded.path, "/v3/profiles");
	}

	#[test]
	fn signed_upload_paths_are_refused() {
		assert!(refuse_signed_path("/v5/media/upload").is_err());
		assert!(refuse_signed_path("/v6/chat/media/upload?takenOnGrindr=true").is_err());
		assert!(refuse_signed_path("/v7/profiles/1").is_ok());
	}

	#[test]
	fn response_round_trips() {
		let encoded = encode_response(&RawResponse { status: 404, body: vec![1, 2, 3] }).unwrap();
		let back: RawResponse = rmp_serde::from_slice(&STANDARD.decode(encoded).unwrap()).unwrap();
		assert_eq!(back.status, 404);
		assert_eq!(back.body, vec![1, 2, 3]);
	}
}
