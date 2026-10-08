//! What Harmonia keeps in its storage folder.
//!
//! ```text
//! import/                 the player puts .hpk files here
//! packs/<hash>.hpk        installed packs, named by their pack hash
//! tables/<hash>.table     the text table of a pack, for the host
//! tables/<hash>.json      what that table was built from (Stamp)
//! translations.json       installed translations (Translation)
//! settings.json           the player's choices (Settings)
//! ```
use angelica_sdk::api::storage;
use serde::{Deserialize, Serialize};

pub const IMPORT: &str = "import";
const LIBRARY: &str = "translations.json";
const SETTINGS: &str = "settings.json";

/// An installed translation, from its pack's manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Translation {
    pub hash: String,
    pub title: String,
    pub team: String,
    pub version: String,
    pub channel: String,
    pub language: String,
    pub game_language: String,
    pub game_version: String,
    /// The fingerprint of the key that signed the pack; `None` for an
    /// unsigned pack the player accepted.
    pub key: Option<String>,
    pub sheets: usize,
    pub cells: usize,
}

impl Translation {
    #[must_use]
    pub fn pack_path(&self) -> String {
        pack_path(&self.hash)
    }
}

#[must_use]
pub fn pack_path(hash: &str) -> String {
    format!("packs/{hash}.hpk")
}

#[must_use]
pub fn table_path(hash: &str) -> String {
    format!("tables/{hash}.table")
}

fn stamp_path(hash: &str) -> String {
    format!("tables/{hash}.json")
}

/// The player's choices. Like every translation choice, they apply at the
/// next game start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// The pack hash of the translation to apply.
    pub active: Option<String>,
    /// Sheets (`Item`) and folders (`quest/*`) kept in the game's language.
    pub kept_sheets: Vec<String>,
}

/// What a table was built from: a table is current while these match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub hash: String,
    pub kept_sheets: Vec<String>,
    pub summary: crate::compile::Summary,
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &str) -> Result<Option<T>, String> {
    match storage::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("{path}: {e}")),
        Err(error) if error.kind == angelica_sdk::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{path}: {}", error.message)),
    }
}

fn write_json(path: &str, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    storage::write(path, &bytes).map_err(|e| format!("{path}: {}", e.message))
}

/// # Errors
/// When the file exists and cannot be read.
pub fn load_library() -> Result<Vec<Translation>, String> {
    Ok(read_json(LIBRARY)?.unwrap_or_default())
}

/// # Errors
/// When the file cannot be written.
pub fn save_library(library: &[Translation]) -> Result<(), String> {
    write_json(LIBRARY, &library)
}

/// # Errors
/// When the file exists and cannot be read.
pub fn load_settings() -> Result<Settings, String> {
    Ok(read_json(SETTINGS)?.unwrap_or_default())
}

/// # Errors
/// When the file cannot be written.
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    write_json(SETTINGS, settings)
}

/// # Errors
/// When the stamp exists and cannot be read.
pub fn load_stamp(hash: &str) -> Result<Option<Stamp>, String> {
    read_json(&stamp_path(hash))
}

/// # Errors
/// When the stamp cannot be written.
pub fn save_stamp(stamp: &Stamp) -> Result<(), String> {
    write_json(&stamp_path(&stamp.hash), stamp)
}

/// Removes a translation's pack and table.
pub fn remove_files(hash: &str) {
    for path in [pack_path(hash), table_path(hash), stamp_path(hash)] {
        let _ = storage::remove(&path);
    }
}

/// The `.hpk` files waiting in `import/`, with their sizes.
#[must_use]
pub fn imports() -> Vec<(String, u64)> {
    storage::list(IMPORT)
        .map(|entries| {
            entries
                .into_iter()
                .filter(|entry| {
                    !entry.directory && entry.name.to_ascii_lowercase().ends_with(".hpk")
                })
                .map(|entry| (format!("{IMPORT}/{}", entry.name), entry.size))
                .collect()
        })
        .unwrap_or_default()
}

/// Reads a whole file in ranges.
///
/// # Errors
/// When the file is missing, larger than `limit`, or unreadable.
pub fn read_file(path: &str, limit: u64) -> Result<Vec<u8>, String> {
    const RANGE: u32 = 8 * 1024 * 1024;
    let size = storage::size(path).map_err(|e| format!("{path}: {}", e.message))?;
    if size > limit {
        return Err(format!("{path} is larger than {limit} bytes"));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).map_err(|e| e.to_string())?);
    while (bytes.len() as u64) < size {
        let remaining = size - bytes.len() as u64;
        let length = u32::try_from(remaining.min(u64::from(RANGE))).unwrap_or(RANGE);
        let part = storage::read_range(path, bytes.len() as u64, length)
            .map_err(|e| format!("{path}: {}", e.message))?;
        bytes.extend_from_slice(&part);
        if size > 0 {
            #[allow(clippy::cast_precision_loss)]
            let fraction = bytes.len() as f32 / size as f32 * 0.3;
            let _ = angelica_sdk::api::jobs::report(fraction, "Reading the pack");
        }
    }
    Ok(bytes)
}
