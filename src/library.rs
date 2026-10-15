//! What Harmonia keeps in its storage folder.
//!
//! ```text
//! import/                 the player puts .hpk files here
//! downloads/              a release from a feed, until it is installed
//! packs/<hash>.hpk        installed packs, named by their pack hash
//! tables/<hash>.table     the text table of a pack, for the host
//! tables/<hash>.json      what that table was built from (Stamp)
//! translations.json       installed translations (Translation)
//! settings.json           the player's choices (Settings)
//! session.json            the number of this game start (Session)
//! dalamud.json            written by Harmonia in Dalamud (Dalamud)
//! ```
use angelica_sdk::api::storage;
use serde::{Deserialize, Serialize};

pub const IMPORT: &str = "import";
const LIBRARY: &str = "translations.json";
const SETTINGS: &str = "settings.json";
const SESSION: &str = "session.json";
const DALAMUD: &str = "dalamud.json";

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
    /// The feed the translation updates from.
    #[serde(default)]
    pub feed: Option<String>,
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

/// Where a patched game file of a translation is kept.
#[must_use]
pub fn font_path(hash: &str, game_path: &str) -> String {
    format!("fonts/{hash}/{game_path}")
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
    /// The feed links Harmonia checks for new versions.
    #[serde(default)]
    pub feeds: Vec<String>,
}

/// What a table was built from: a table is current while these match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub hash: String,
    pub kept_sheets: Vec<String>,
    pub summary: crate::compile::Summary,
    /// The game build the font files were made from.
    pub game_version: String,
    /// Patched game font files: game path and storage path.
    pub fonts: Vec<(String, String)>,
    pub font_reports: Vec<crate::font_patch::Report>,
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

/// The number of the last game start with Harmonia.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Session {
    session: u64,
}

/// What Harmonia in Dalamud writes when it translates the game: the game
/// build and the number of the session it found in `session.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Dalamud {
    translating: bool,
    game_version: String,
    angelica_session: u64,
}

/// Counts a game start and tells whether Harmonia in Dalamud translated the
/// game in the previous session on the same game build. When it did, this
/// copy stays off: the game is translated once, and Dalamud's build also
/// keeps what its other plugins read. Once Dalamud stops loading (after a
/// game update, say), Harmonia in Dalamud no longer answers and this copy
/// translates from the next start. A damaged `dalamud.json` counts as no
/// answer.
///
/// # Errors
/// When `session.json` cannot be read or written.
pub fn begin_session(game_version: &str) -> Result<bool, String> {
    let previous = read_json::<Session>(SESSION)?.unwrap_or_default().session;
    write_json(
        SESSION,
        &Session {
            session: previous + 1,
        },
    )?;
    let dalamud = read_json::<Dalamud>(DALAMUD).ok().flatten();
    Ok(previous > 0
        && dalamud.is_some_and(|d| {
            d.translating && d.game_version == game_version && d.angelica_session == previous
        }))
}

/// Removes a translation's pack, table, and font files.
pub fn remove_files(hash: &str) {
    if let Ok(Some(stamp)) = load_stamp(hash) {
        for (_, path) in &stamp.fonts {
            let _ = storage::remove(path);
        }
    }
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

/// A pack in `downloads/` that a game downloaded and did not install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub path: String,
    /// The feed it came from.
    pub feed: String,
    /// The key fingerprint that feed declares.
    pub key: Option<String>,
}

/// Records the feed a pack in `downloads/` came from and the key it
/// declares, so a pack a game downloaded and did not install is picked up
/// at the next start.
///
/// # Errors
/// When the record cannot be written.
pub fn remember_download(hash: &str, feed: &str, key: Option<&str>) -> Result<(), String> {
    let record = serde_json::json!({ "feed": feed, "key": key }).to_string();
    storage::write(&format!("downloads/{hash}.json"), record.as_bytes()).map_err(|e| e.message)
}

/// Forgets a downloaded pack: the pack if it is still there, and every
/// record whose pack is gone.
pub fn forget_download(hash: &str) {
    let _ = storage::remove(&format!("downloads/{hash}.hpk"));
    let Ok(entries) = storage::list("downloads") else {
        return;
    };
    let names: Vec<String> = entries.into_iter().map(|entry| entry.name).collect();
    for record in names.iter().filter_map(|name| name.strip_suffix(".json")) {
        if !names.iter().any(|name| *name == format!("{record}.hpk")) {
            let _ = storage::remove(&format!("downloads/{record}.json"));
        }
    }
}

/// The packs in `downloads/` that a record names a feed for.
#[must_use]
pub fn downloaded() -> Vec<Download> {
    #[derive(Deserialize)]
    struct Record {
        feed: String,
        #[serde(default)]
        key: Option<String>,
    }
    let Ok(entries) = storage::list("downloads") else {
        return Vec::new();
    };
    let names: Vec<String> = entries.into_iter().map(|entry| entry.name).collect();
    names
        .iter()
        .filter_map(|name| name.strip_suffix(".json"))
        .filter(|hash| names.iter().any(|name| *name == format!("{hash}.hpk")))
        .filter_map(|hash| {
            let bytes = storage::read(&format!("downloads/{hash}.json")).ok()?;
            let record: Record = serde_json::from_slice(&bytes).ok()?;
            Some(Download {
                path: format!("downloads/{hash}.hpk"),
                feed: record.feed,
                key: record.key,
            })
        })
        .collect()
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
            let _ = angelica_sdk::api::jobs::report(
                fraction,
                &crate::words("Reading the pack", "Чтение пака"),
            );
        }
    }
    Ok(bytes)
}
