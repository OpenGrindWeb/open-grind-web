//! Port of `src-tauri/src/photo/mod.rs` without the Tauri handle.
//!
//! `source.rs` and `encode.rs` are compiled straight from the upstream tree.
//! HEIC decoding is Android-only upstream; on iPhone, Safari's file picker
//! already hands over JPEG when the input accepts `image/*`.

#[path = "../../src-tauri/src/photo/encode.rs"]
mod encode;
#[path = "../../src-tauri/src/photo/source.rs"]
pub(crate) mod source;

use crate::error::AppError;

pub use encode::Thumbnail;
use source::Format;

pub const JPEG: &str = "image/jpeg";
const DAMAGED: &str = "Unreadable or damaged photo";
const HEIF_UNSUPPORTED: &str = "HEIC photos aren't supported here: pick the photo again from the Photos library";

pub struct Photo {
	pub bytes: Vec<u8>,
	pub content_type: String,
}

pub async fn normalize(bytes: Vec<u8>, content_type: String) -> Result<Photo, AppError> {
	let Some(format) = source::sniff(&bytes) else {
		return Ok(Photo { bytes, content_type });
	};
	let source = decode(format, bytes).await?;
	let bytes = off_thread(move || encode::to_jpeg(source)).await?.ok_or_else(damaged)?;
	Ok(Photo {
		bytes,
		content_type: JPEG.to_owned(),
	})
}

pub async fn thumbnail(bytes: Vec<u8>) -> Result<Option<Thumbnail>, AppError> {
	let Some(format) = source::sniff(&bytes) else {
		return Ok(None);
	};
	let source = decode(format, bytes).await?;
	off_thread(move || encode::to_thumbnail(source))
		.await?
		.ok_or_else(damaged)
		.map(Some)
}

async fn decode(format: Format, bytes: Vec<u8>) -> Result<source::Source, AppError> {
	if format == Format::Heif {
		return Err(AppError::Media(HEIF_UNSUPPORTED.to_owned()));
	}
	off_thread(move || source::decode(format, &bytes)).await?.ok_or_else(damaged)
}

fn damaged() -> AppError {
	AppError::Media(DAMAGED.to_owned())
}

pub(crate) async fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, AppError> {
	tokio::task::spawn_blocking(work)
		.await
		.map_err(|error| AppError::Media(error.to_string()))
}
