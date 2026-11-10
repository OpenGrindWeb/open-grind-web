//! File-backed replacement for the platform keyring (`src-tauri/src/storage`).
//!
//! One msgpack file per entry, written atomically with owner-only
//! permissions. Entry names match `storage/entries.rs` so a later migration
//! from a desktop install stays possible.
//!
//! TODO(phase 6): encrypt at rest with a key that is not stored next to the
//! data.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

pub const DEVICE_INFO: &str = "device-info";
pub const SESSION: &str = "session";
pub const SIGNING_KEY: &str = "device-signing-key";

#[derive(Clone)]
pub struct SecretStore {
	dir: PathBuf,
}

impl SecretStore {
	pub fn open(dir: &Path) -> std::io::Result<Self> {
		let dir = dir.join("secrets");
		std::fs::create_dir_all(&dir)?;
		restrict(&dir, 0o700)?;
		Ok(Self { dir })
	}

	fn path(&self, name: &str) -> PathBuf {
		self.dir.join(format!("{name}.msgpack"))
	}

	pub fn load<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, String> {
		let bytes = match std::fs::read(self.path(name)) {
			Ok(b) => b,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
			Err(e) => return Err(format!("read {name}: {e}")),
		};
		rmp_serde::from_slice(&bytes)
			.map(Some)
			.map_err(|e| format!("decode {name}: {e}"))
	}

	pub fn save<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
		let bytes = rmp_serde::encode::to_vec_named(value)
			.map_err(|e| format!("encode {name}: {e}"))?;
		let target = self.path(name);
		let tmp = target.with_extension("tmp");
		std::fs::write(&tmp, &bytes).map_err(|e| format!("write {name}: {e}"))?;
		restrict(&tmp, 0o600).map_err(|e| format!("chmod {name}: {e}"))?;
		std::fs::rename(&tmp, &target).map_err(|e| format!("rename {name}: {e}"))
	}

	pub fn delete(&self, name: &str) {
		match std::fs::remove_file(self.path(name)) {
			Ok(()) => {}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
			Err(e) => tracing::warn!("[store] could not delete {name}: {e}"),
		}
	}

	/// Port of `DeviceStorage::load_or_create`.
	pub fn device_or_create(&self) -> grindr::DeviceInfo {
		match self.load::<grindr::DeviceInfo>(DEVICE_INFO) {
			Ok(Some(device)) => return device,
			Ok(None) => {}
			Err(e) => tracing::warn!("[store] replacing unreadable device info: {e}"),
		}
		let device = grindr::DeviceInfo::generate();
		if let Err(e) = self.save(DEVICE_INFO, &device) {
			tracing::error!("[store] could not persist device info: {e}");
		}
		device
	}
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
	use std::os::unix::fs::PermissionsExt;
	std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trips_and_deletes() {
		let dir = std::env::temp_dir().join(format!("og-store-{}", std::process::id()));
		let store = SecretStore::open(&dir).unwrap();
		assert_eq!(store.load::<String>("x").unwrap(), None);
		store.save("x", &"hello".to_owned()).unwrap();
		assert_eq!(store.load::<String>("x").unwrap().as_deref(), Some("hello"));
		store.delete("x");
		assert_eq!(store.load::<String>("x").unwrap(), None);
		let _ = std::fs::remove_dir_all(dir);
	}
}
