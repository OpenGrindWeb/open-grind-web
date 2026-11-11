//! Port of `src-tauri/src/upload` for the web build.
//!
//! The bridge answers `plugin:dialog|open` with Safari's file picker, sends
//! each chosen file to `POST /stage`, and hands the front an opaque path
//! (`/staged/<uuid>.<ext>`). The front then treats it like a desktop file:
//! `inspect_media_file`, `preview_media`, `upload_media_file` and
//! `upload_media` receive `{ source: "desktop", path }` and run here against
//! the staged copy, with upstream's photo normalization (re-encoded JPEG, no
//! EXIF) and video metadata stripping.
//!
//! A path only ever resolves inside the staging directory: anything that is
//! not exactly `/staged/<uuid>.<known ext>` is refused.

#[path = "../../src-tauri/src/upload/form.rs"]
mod form;

use std::fs::File;
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::body::Bytes as AxumBytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use grindr::{BodySource, Bytes, Session};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use self::form::{FormPart, Framing};
use crate::error::AppError;
use crate::hex::hex;
use crate::ipc::{encode_response, refuse_signed_path, RawResponse};
use crate::photo::{self, source};
use crate::video::strip::{self, Patch, PatchedReader};
use crate::video::{boxes, probe, sniff};
use crate::AppState;

const MAX_PHOTO_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_STAGED_BYTES: usize = 512 * 1024 * 1024;
const STAGED_TTL: Duration = Duration::from_secs(6 * 3600);
const SNIFF_LEN: usize = 64;
const VIDEO_MP4: &str = "video/mp4";
const NOT_MEDIA: &str = "That file is not a photo or a video";
const NOT_A_PHOTO: &str = "Only photos can be sent here";
const VIDEO_UNREADABLE: &str = "That video's format can't be read";
const UNREADABLE_SIZE: &str = "That photo's size can't be read";
const STAGED_PREFIX: &str = "/staged/";
const EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "webp", "mp4", "mov"];

static UPLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn media_error(error: io::Error) -> AppError {
	AppError::Media(error.to_string())
}

// --- staging -----------------------------------------------------------------

#[derive(Clone)]
pub struct Staging {
	dir: PathBuf,
}

impl Staging {
	pub fn open(data_dir: &Path) -> io::Result<Self> {
		let dir = data_dir.join("staged");
		std::fs::create_dir_all(&dir)?;
		let staging = Self { dir };
		staging.purge(Duration::ZERO);
		Ok(staging)
	}

	/// Drops staged files older than `age` (all of them at startup).
	pub fn purge(&self, age: Duration) {
		let Ok(entries) = std::fs::read_dir(&self.dir) else {
			return;
		};
		let now = SystemTime::now();
		for entry in entries.flatten() {
			let old = entry
				.metadata()
				.and_then(|m| m.modified())
				.map(|modified| now.duration_since(modified).unwrap_or_default() >= age)
				.unwrap_or(true);
			if old {
				let _ = std::fs::remove_file(entry.path());
			}
		}
	}

	pub fn spawn_janitor(&self) {
		let staging = self.clone();
		tokio::spawn(async move {
			let mut tick = tokio::time::interval(Duration::from_secs(600));
			loop {
				tick.tick().await;
				staging.purge(STAGED_TTL);
			}
		});
	}

	/// `/staged/<uuid>.<ext>` -> file in the staging dir; anything else: None.
	fn resolve(&self, path: &str) -> Option<PathBuf> {
		let name = path.strip_prefix(STAGED_PREFIX)?;
		let (stem, ext) = name.rsplit_once('.')?;
		let valid_stem = stem.len() == 36 && stem.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
		(valid_stem && EXTENSIONS.contains(&ext)).then(|| self.dir.join(name))
	}
}

fn extension_for(content_type: &str, file_name: &str) -> Option<&'static str> {
	let by_type = match content_type.split(';').next().unwrap_or("").trim() {
		"image/jpeg" => Some("jpg"),
		"image/png" => Some("png"),
		"image/webp" => Some("webp"),
		"video/mp4" => Some("mp4"),
		"video/quicktime" => Some("mov"),
		_ => None,
	};
	by_type.or_else(|| {
		let ext = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
		EXTENSIONS.iter().copied().find(|known| *known == ext)
	})
}

#[derive(Serialize)]
pub struct Staged {
	path: String,
}

/// `POST /stage`: raw file body, `content-type` and `x-og-filename` headers.
pub async fn stage(State(app): State<AppState>, headers: HeaderMap, body: AxumBytes) -> Response {
	let content_type = headers
		.get(axum::http::header::CONTENT_TYPE)
		.and_then(|v| v.to_str().ok())
		.unwrap_or("");
	let file_name = headers.get("x-og-filename").and_then(|v| v.to_str().ok()).unwrap_or("");
	let Some(ext) = extension_for(content_type, file_name) else {
		return (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(AppError::Media(NOT_MEDIA.to_owned()))).into_response();
	};
	let name = format!("{}.{ext}", uuid::Uuid::new_v4().hyphenated());
	let target = app.staging.dir.join(&name);
	if let Err(e) = tokio::fs::write(&target, &body).await {
		tracing::error!("[upload] staging failed: {e}");
		return (StatusCode::INTERNAL_SERVER_ERROR, Json(AppError::Media(e.to_string()))).into_response();
	}
	Json(Staged {
		path: format!("{STAGED_PREFIX}{name}"),
	})
	.into_response()
}

// --- picked files ------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum PickedFile {
	Desktop { path: String },
	Staged { id: String },
}

impl PickedFile {
	fn resolve(&self, staging: &Staging) -> Result<PathBuf, AppError> {
		let path = match self {
			PickedFile::Desktop { path } => path.clone(),
			// `stage_proxied_photo` hands out the staged file name as its id.
			PickedFile::Staged { id } => format!("{STAGED_PREFIX}{id}"),
		};
		staging
			.resolve(&path)
			.ok_or_else(|| AppError::Media("That file is no longer available, pick it again".to_owned()))
	}
}

// --- inspection (port of upload/inspect.rs) -------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
	Photo,
	Video,
	Unsupported,
}

fn sniff_kind(head: &[u8]) -> MediaKind {
	if source::sniff(head).is_some() {
		MediaKind::Photo
	} else if sniff::is_video(head) {
		MediaKind::Video
	} else {
		MediaKind::Unsupported
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
	pub kind: MediaKind,
	pub size: u64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub width: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub height: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub duration_ms: Option<u64>,
}

fn inspect(file: &mut File) -> io::Result<Inspection> {
	let size = file.metadata()?.len();
	let mut head = [0u8; SNIFF_LEN];
	let filled = boxes::read_up_to(file, &mut head)?;
	let head = &head[..filled];
	let bare = |kind| Inspection {
		kind,
		size,
		width: None,
		height: None,
		duration_ms: None,
	};
	Ok(match sniff_kind(head) {
		MediaKind::Photo => bare(MediaKind::Photo),
		MediaKind::Video => match probe::probe(file).ok().flatten() {
			Some(probed) => Inspection {
				width: Some(probed.width),
				height: Some(probed.height),
				duration_ms: probed.duration_ms,
				..bare(MediaKind::Video)
			},
			None => bare(MediaKind::Unsupported),
		},
		MediaKind::Unsupported => bare(MediaKind::Unsupported),
	})
}

fn inspect_as_shown(file: &mut File) -> io::Result<Inspection> {
	let inspection = inspect(file)?;
	if inspection.kind != MediaKind::Photo {
		return Ok(inspection);
	}
	let upright = photo_dimensions(file);
	Ok(Inspection {
		width: upright.map(|(width, _)| width),
		height: upright.map(|(_, height)| height),
		..inspection
	})
}

fn photo_dimensions(file: &mut File) -> Option<(u32, u32)> {
	file.seek(SeekFrom::Start(0)).ok()?;
	let mut head = [0u8; SNIFF_LEN];
	let filled = boxes::read_up_to(file, &mut head).ok()?;
	let format = source::sniff(&head[..filled])?;
	file.seek(SeekFrom::Start(0)).ok()?;
	source::dimensions(format, BufReader::new(file))
}

#[derive(Deserialize)]
pub struct FileArgs {
	file: PickedFile,
}

pub async fn inspect_media_file(app: &AppState, args: FileArgs) -> Result<Inspection, AppError> {
	let path = args.file.resolve(&app.staging)?;
	photo::off_thread(move || inspect_as_shown(&mut File::open(path)?))
		.await?
		.map_err(media_error)
}

// --- preview (port of upload/preview.rs) -------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
	data_url: String,
	width: u32,
	height: u32,
}

fn photo_too_large() -> String {
	format!("Photos over {} MB can't be uploaded", MAX_PHOTO_BYTES / 1024 / 1024)
}

fn read_photo(file: &mut File, size: u64) -> Result<Vec<u8>, AppError> {
	if size > MAX_PHOTO_BYTES {
		return Err(AppError::Media(photo_too_large()));
	}
	file.seek(SeekFrom::Start(0)).map_err(media_error)?;
	let mut bytes = Vec::with_capacity(size.min(MAX_PHOTO_BYTES) as usize);
	file.take(MAX_PHOTO_BYTES + 1).read_to_end(&mut bytes).map_err(media_error)?;
	if bytes.len() as u64 > MAX_PHOTO_BYTES {
		return Err(AppError::Media(photo_too_large()));
	}
	Ok(bytes)
}

async fn open_photo(path: PathBuf) -> Result<Option<Vec<u8>>, AppError> {
	photo::off_thread(move || {
		let mut file = File::open(path).map_err(media_error)?;
		let inspection = inspect(&mut file).map_err(media_error)?;
		if inspection.kind != MediaKind::Photo {
			return Ok(None);
		}
		read_photo(&mut file, inspection.size).map(Some)
	})
	.await?
}

pub async fn preview_media(app: &AppState, args: FileArgs) -> Result<Option<Preview>, AppError> {
	let path = args.file.resolve(&app.staging)?;
	let Some(bytes) = open_photo(path).await? else {
		return Ok(None);
	};
	let Some(thumbnail) = photo::thumbnail(bytes).await? else {
		return Ok(None);
	};
	Ok(Some(Preview {
		data_url: format!("data:{};base64,{}", photo::JPEG, STANDARD.encode(&thumbnail.jpeg)),
		width: thumbnail.width,
		height: thumbnail.height,
	}))
}

// --- session guard (from upload/file.rs) ---------------------------------------

fn profile_of(session: &Option<Session>) -> Option<&str> {
	session.as_ref().and_then(|session| session.credentials.profile_id.as_deref())
}

fn signed_in_as(session: &Option<Session>, profile_id: &str) -> bool {
	profile_of(session) == Some(profile_id)
}

async fn unless_session_changes<T>(
	sessions: &mut tokio::sync::watch::Receiver<Option<Session>>,
	profile_id: &str,
	send: impl std::future::Future<Output = T>,
) -> Result<T, AppError> {
	tokio::select! {
		biased;
		_ = sessions.wait_for(|session| !signed_in_as(session, profile_id)) => Err(AppError::SessionCleared),
		sent = send => Ok(sent),
	}
}

// --- upload_media_file (port of upload/file.rs) --------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartSpec {
	pub name: String,
	pub filename: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRequest {
	pub method: String,
	pub path: String,
	#[serde(default)]
	pub part: Option<PartSpec>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadOutcome {
	pub response: String,
	pub sha256: Option<String>,
	pub body_size: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadMediaFileArgs {
	file: PickedFile,
	request: UploadRequest,
	max_body_size: u64,
	profile_id: String,
}

fn framing_for(part: Option<&PartSpec>, content_type: &str) -> Framing {
	match part {
		Some(part) => Framing::new(&FormPart {
			name: &part.name,
			filename: &part.filename,
			content_type,
		}),
		None => Framing::raw(content_type),
	}
}

enum Source {
	Photo { bytes: Vec<u8> },
	Video { size: u64, patches: Arc<[Patch]> },
}

fn read_source(path: &Path) -> Result<Source, AppError> {
	let mut file = File::open(path).map_err(media_error)?;
	let inspection = inspect(&mut file).map_err(media_error)?;
	match inspection.kind {
		MediaKind::Photo => Ok(Source::Photo {
			bytes: read_photo(&mut file, inspection.size)?,
		}),
		MediaKind::Video => {
			let uploaded_at = strip::movie_time(SystemTime::now());
			let patches = strip::plan(&mut file, uploaded_at)
				.map_err(media_error)?
				.ok_or_else(|| AppError::Media(VIDEO_UNREADABLE.to_owned()))?;
			Ok(Source::Video {
				size: inspection.size,
				patches: Arc::from(patches),
			})
		}
		MediaKind::Unsupported => Err(AppError::Media(NOT_MEDIA.to_owned())),
	}
}

/// Streams a staged video with its metadata patched out, hashing as it goes
/// (port of upload/stream.rs `FileBody`).
struct VideoBody {
	path: PathBuf,
	framing: Framing,
	content_len: u64,
	patches: Arc<[Patch]>,
	digest: Arc<Mutex<Option<[u8; 32]>>>,
}

struct HashingReader<R> {
	inner: R,
	hasher: Sha256,
	read: u64,
	expected: u64,
	digest: Arc<Mutex<Option<[u8; 32]>>>,
}

impl<R: Read> Read for HashingReader<R> {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		let read = self.inner.read(buf)?;
		self.hasher.update(&buf[..read]);
		self.read += read as u64;
		if read == 0 || self.read == self.expected {
			if self.read != self.expected {
				return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the file changed while uploading"));
			}
			*self.digest.lock().unwrap() = Some(self.hasher.clone().finalize().into());
		}
		Ok(read)
	}
}

impl BodySource for VideoBody {
	fn size(&self) -> u64 {
		self.framing.size(self.content_len)
	}

	fn open(&self) -> io::Result<Box<dyn Read + Send>> {
		let file = File::open(&self.path)?;
		*self.digest.lock().unwrap() = None;
		let content = HashingReader {
			inner: PatchedReader::new(file.take(self.content_len), Arc::clone(&self.patches)),
			hasher: Sha256::new(),
			read: 0,
			expected: self.content_len,
			digest: Arc::clone(&self.digest),
		};
		Ok(Box::new(
			Cursor::new(self.framing.head.clone())
				.chain(content)
				.chain(Cursor::new(self.framing.tail.clone())),
		))
	}
}

pub async fn upload_media_file(app: &AppState, args: UploadMediaFileArgs) -> Result<UploadOutcome, AppError> {
	let client = app.client()?;
	refuse_signed_path(&args.request.path)?;
	let method: grindr::Method = args.request.method.parse().map_err(|_| AppError::Api {
		code: 400,
		message: format!("Invalid method: {}", args.request.method),
	})?;
	let path = args.file.resolve(&app.staging)?;
	let _one_at_a_time = UPLOAD_LOCK.lock().await;

	let source = {
		let path = path.clone();
		photo::off_thread(move || read_source(&path)).await??
	};

	let mut sessions = client.session_receiver();
	if !signed_in_as(&sessions.borrow(), &args.profile_id) {
		return Err(AppError::SessionCleared);
	}
	let builder = client.request(method, &args.request.path);

	let (send, body_size, sha256) = match source {
		Source::Photo { bytes } => {
			let photo = photo::normalize(bytes, photo::JPEG.to_owned()).await?;
			let framing = framing_for(args.request.part.as_ref(), &photo.content_type);
			let body_size = framing.size(photo.bytes.len() as u64);
			if body_size > args.max_body_size {
				return Err(AppError::ContentTooLarge);
			}
			let sha256 = hex(&Sha256::digest(&photo.bytes));
			let body = framing.frame(Bytes::from(photo.bytes));
			let content_type = framing.content_type.clone();
			(builder.bytes(&content_type, body).send(), body_size, Ok(sha256))
		}
		Source::Video { size, patches } => {
			let framing = framing_for(args.request.part.as_ref(), VIDEO_MP4);
			let body_size = framing.size(size);
			if body_size > args.max_body_size {
				return Err(AppError::ContentTooLarge);
			}
			let digest = Arc::new(Mutex::new(None));
			let content_type = framing.content_type.clone();
			let body = VideoBody {
				path,
				framing,
				content_len: size,
				patches,
				digest: Arc::clone(&digest),
			};
			(builder.stream(&content_type, body).send(), body_size, Err(digest))
		}
	};

	let raw = unless_session_changes(&mut sessions, &args.profile_id, send)
		.await?
		.map_err(|error| AppError::from_client_error(error, &client))?;

	let sha256 = match sha256 {
		Ok(known) => Some(known),
		Err(digest) => digest.lock().unwrap().map(|d| hex(&d)),
	};
	Ok(UploadOutcome {
		response: encode_response(&RawResponse {
			status: raw.status,
			body: raw.body,
		})?,
		sha256,
		body_size,
	})
}

// --- upload_media (port of upload/bytes.rs) ------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadMediaArgs {
	path: String,
	signed: bool,
	file: PickedFile,
	/// Upstream PR 523 replaced `squareThumb: true` with `crop`; both are
	/// accepted.
	#[serde(default)]
	crop: Option<PhotoCrop>,
	#[serde(default)]
	square_thumb: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PhotoCrop {
	ThumbCoords,
	CropAndThumb,
}

/// Port of `with_crop` (upload/bytes.rs, upstream PR 523).
fn with_crop(path: &str, jpeg: &[u8], crop: PhotoCrop) -> Result<String, AppError> {
	use image::{ImageFormat, ImageReader};
	let (width, height) = ImageReader::with_format(Cursor::new(jpeg), ImageFormat::Jpeg)
		.into_dimensions()
		.map_err(|_| AppError::Media(UNREADABLE_SIZE.to_owned()))?;
	let side = width.min(height);
	let left = (width - side) / 2;
	let top = (height - side) / 2;
	let (right, bottom) = (left + side, top + side);
	let query = match crop {
		PhotoCrop::ThumbCoords => format!("thumbCoords={bottom},{left},{right},{top}"),
		PhotoCrop::CropAndThumb => format!(
			"crop_top=0&crop_bottom={height}&crop_left=0&crop_right={width}&img_1_top={top}&img_1_bottom={bottom}&img_1_left={left}&img_1_right={right}&rotate=0&flip_vertical=false"
		),
	};
	let separator = if path.contains('?') { '&' } else { '?' };
	Ok(format!("{path}{separator}{query}"))
}

#[cfg(test)]
fn with_square_thumb(path: &str, jpeg: &[u8]) -> Result<String, AppError> {
	with_crop(path, jpeg, PhotoCrop::ThumbCoords)
}

// --- stage_proxied_photo (upload/staged.rs, upstream PR 523) -------------------

const UNREADABLE_PHOTO: &str = "That photo couldn't be loaded";

#[derive(Deserialize)]
pub struct StageProxiedPhotoArgs {
	url: String,
}

/// Fetches a photo already on Grindr's CDN (an earlier chat upload) and
/// stages it so it can be uploaded again; returns the staged id.
pub async fn stage_proxied_photo(app: &AppState, args: StageProxiedPhotoArgs) -> Result<String, AppError> {
	let unreadable = || AppError::Media(UNREADABLE_PHOTO.to_owned());
	if !args.url.starts_with("https://") {
		return Err(unreadable());
	}
	let client = app.client()?;
	let media = client
		.fetch_media(grindr::MediaRequest {
			url: &args.url,
			range: None,
			max_bytes: MAX_PHOTO_BYTES as usize,
			fetcher: grindr::MediaFetcher::ImageLoader,
		})
		.await
		.map_err(|_| unreadable())?;
	if !(200..300).contains(&media.status) {
		return Err(unreadable());
	}
	let ext = match source::sniff(&media.body) {
		Some(source::Format::Jpeg) => "jpg",
		Some(source::Format::Png) => "png",
		Some(source::Format::Webp) => "webp",
		_ => return Err(unreadable()),
	};
	let id = format!("{}.{ext}", uuid::Uuid::new_v4().hyphenated());
	tokio::fs::write(app.staging.dir.join(&id), &media.body)
		.await
		.map_err(media_error)?;
	Ok(id)
}

pub async fn upload_media(app: &AppState, args: UploadMediaArgs) -> Result<String, AppError> {
	let client = app.client()?;
	let mut sessions = client.session_receiver();
	let Some(profile_id) = profile_of(&sessions.borrow()).map(str::to_owned) else {
		return Err(AppError::NotSignedIn);
	};
	let staged = args.file.resolve(&app.staging)?;
	let Some(bytes) = open_photo(staged).await? else {
		return Err(AppError::Media(NOT_A_PHOTO.to_owned()));
	};
	let photo = photo::normalize(bytes, photo::JPEG.to_owned()).await?;
	let crop = args
		.crop
		.or(args.square_thumb.unwrap_or(false).then_some(PhotoCrop::ThumbCoords));
	let path = match crop {
		Some(crop) => with_crop(&args.path, &photo.bytes, crop)?,
		None => args.path,
	};
	if !signed_in_as(&sessions.borrow(), &profile_id) {
		return Err(AppError::SessionCleared);
	}
	let request = client.request(grindr::Method::POST, &path);
	let request = if args.signed {
		request.signed_bytes(&photo.content_type, photo.bytes)
	} else {
		request.bytes(&photo.content_type, photo.bytes)
	};
	let raw = unless_session_changes(&mut sessions, &profile_id, request.send())
		.await?
		.map_err(|e| AppError::from_client_error(e, &client))?;
	encode_response(&RawResponse {
		status: raw.status,
		body: raw.body,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn staging() -> (Staging, PathBuf) {
		let dir = std::env::temp_dir().join(format!("og-staging-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
		(Staging::open(&dir).unwrap(), dir)
	}

	#[test]
	fn only_staged_uuid_names_resolve() {
		let (staging, dir) = staging();
		let ok = "/staged/0f8c2a4e-1b2c-4d5e-8f90-123456789abc.jpg";
		assert!(staging.resolve(ok).unwrap().starts_with(&staging.dir));
		for bad in [
			"/staged/../secrets/session.msgpack",
			"/staged/0f8c2a4e-1b2c-4d5e-8f90-123456789abc.exe",
			"/etc/passwd",
			"/staged/short.jpg",
			"/staged/0f8c2a4e-1b2c-4d5e-8f90-123456789abc/../x.jpg",
		] {
			assert!(staging.resolve(bad).is_none(), "{bad}");
		}
		let _ = std::fs::remove_dir_all(dir);
	}

	#[test]
	fn extension_comes_from_type_then_name() {
		assert_eq!(extension_for("image/jpeg", ""), Some("jpg"));
		assert_eq!(extension_for("video/quicktime", "x.MOV"), Some("mov"));
		assert_eq!(extension_for("", "IMG_1.PNG"), Some("png"));
		assert_eq!(extension_for("image/heic", "IMG_1.HEIC"), None);
		assert_eq!(extension_for("text/html", "x.html"), None);
	}

	#[test]
	fn square_thumb_is_centered() {
		let mut jpeg = Vec::new();
		image::DynamicImage::new_rgb8(768, 1024)
			.write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
			.unwrap();
		assert_eq!(
			with_square_thumb("/v4/media/upload?takenOnGrindr=false", &jpeg).unwrap(),
			"/v4/media/upload?takenOnGrindr=false&thumbCoords=896,0,768,128"
		);
	}

	#[test]
	fn crop_and_thumb_matches_upstream() {
		let mut jpeg = Vec::new();
		image::DynamicImage::new_rgb8(768, 1024)
			.write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
			.unwrap();
		assert_eq!(
			with_crop("/v2/media/upload", &jpeg, PhotoCrop::CropAndThumb).unwrap(),
			"/v2/media/upload?crop_top=0&crop_bottom=1024&crop_left=0&crop_right=768&img_1_top=128&img_1_bottom=896&img_1_left=0&img_1_right=768&rotate=0&flip_vertical=false"
		);
		let args: UploadMediaArgs = serde_json::from_value(serde_json::json!({
			"path": "/p", "signed": false, "file": {"source": "staged", "id": "x"}, "crop": "cropAndThumb"
		}))
		.unwrap();
		assert_eq!(args.crop, Some(PhotoCrop::CropAndThumb));
	}

	#[test]
	fn a_staged_id_resolves_like_its_path() {
		let (staging, dir) = staging();
		let id = "0f8c2a4e-1b2c-4d5e-8f90-123456789abc.jpg";
		let picked = PickedFile::Staged { id: id.to_owned() };
		assert_eq!(picked.resolve(&staging).unwrap(), staging.dir.join(id));
		assert!(PickedFile::Staged { id: "../x.jpg".to_owned() }.resolve(&staging).is_err());
		let _ = std::fs::remove_dir_all(dir);
	}

	#[tokio::test]
	async fn a_photo_is_inspected_previewed_and_normalized() {
		let mut jpeg = Vec::new();
		image::DynamicImage::new_rgb8(800, 600)
			.write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
			.unwrap();
		let (staging, dir) = staging();
		let path = staging.dir.join("0f8c2a4e-1b2c-4d5e-8f90-123456789abc.jpg");
		std::fs::write(&path, &jpeg).unwrap();

		let inspection = inspect_as_shown(&mut File::open(&path).unwrap()).unwrap();
		assert_eq!(inspection.kind, MediaKind::Photo);
		assert_eq!((inspection.width, inspection.height), (Some(800), Some(600)));

		let thumb = photo::thumbnail(jpeg.clone()).await.unwrap().unwrap();
		assert!(thumb.width <= 800 && !thumb.jpeg.is_empty());

		let normalized = photo::normalize(jpeg, photo::JPEG.to_owned()).await.unwrap();
		assert_eq!(normalized.content_type, photo::JPEG);
		let _ = std::fs::remove_dir_all(dir);
	}
}
