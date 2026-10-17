//! Translation feeds: a publisher's list of releases (Aeria
//! `docs/formats/feed-v1.md`).
//!
//! A feed is only a hint. Every fact used for a decision is checked again
//! against the downloaded pack, which is verified like any other pack.
//! Unknown fields are ignored by contract.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;

/// The largest feed document read.
pub const MAX_FEED: usize = 1 << 20;

/// Where a download waits to be checked.
pub const DOWNLOAD: &str = "downloads/pack.download";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub channel: String,
    pub language: String,
    pub game_language: String,
    pub game_version: String,
    /// The lowest Harmonia version that reads the pack.
    pub min_harmonia: Vec<u32>,
    /// The pack hash, 64 hex digits.
    pub pack_hash: String,
    pub url: String,
    /// Whether the download is the pack compressed with Brotli.
    pub brotli: bool,
    pub size: u64,
    /// SHA-256 of the downloaded file, 64 hex digits.
    pub sha256: String,
    pub unpacked_size: u64,
    /// What the publisher says is new.
    #[serde(default)]
    pub changelog: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub title: Option<String>,
    /// The fingerprint of the key the publisher says it signs with.
    pub key: Option<String>,
    pub releases: Vec<Release>,
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn string(object: &Value, field: &str) -> Result<String, String> {
    match object.get(field).and_then(Value::as_str).map(str::trim) {
        Some(text) if !text.is_empty() => Ok(text.to_owned()),
        _ => Err(format!("the feed field {field} must be a non-empty string")),
    }
}

fn optional(object: &Value, field: &str) -> Result<Option<String>, String> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => string(object, field).map(Some),
    }
}

fn integer(object: &Value, field: &str) -> Result<u64, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("the feed field {field} must be a non-negative integer"))
}

/// A version of one to four numbers, such as `0.1.2.0`.
fn numbers(version: &str) -> Option<Vec<u32>> {
    let parts: Option<Vec<u32>> = version.split('.').map(|part| part.parse().ok()).collect();
    parts.filter(|parts| (1..=4).contains(&parts.len()))
}

/// Whether `url` is an `https://` link Harmonia may request.
#[must_use]
pub fn is_link(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty()
            && !rest.starts_with('/')
            && url.len() <= 1024
            && !url.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}

fn release(value: &Value) -> Result<Release, String> {
    if !value.is_object() {
        return Err("a feed release must be an object".into());
    }
    let game = value
        .get("game")
        .filter(|game| game.is_object())
        .ok_or("a feed release has no game")?;
    let download = value
        .get("download")
        .filter(|download| download.is_object())
        .ok_or("a feed release has no download")?;
    let version = string(value, "version")?;
    if !crate::pack::valid_version(&version) {
        return Err("a feed release has an invalid version".into());
    }
    let channel = string(value, "channel")?;
    if channel != "stable" && channel != "testing" {
        return Err("a feed release has an invalid channel".into());
    }
    let pack_hash = string(value, "packHash")?;
    let pack_hash = pack_hash
        .strip_prefix("sha256:")
        .filter(|hash| is_sha256(hash))
        .ok_or("a feed release has an invalid packHash")?
        .to_owned();
    let min_harmonia = numbers(&string(value, "minHarmonia")?)
        .ok_or("a feed release has an invalid minHarmonia")?;
    let url = string(download, "url")?;
    if !is_link(&url) {
        return Err("a feed release must download from an https link".into());
    }
    let brotli = match string(download, "encoding")?.as_str() {
        "br" => true,
        "identity" => false,
        _ => return Err("a feed release has an unsupported download encoding".into()),
    };
    let sha256 = string(download, "sha256")?;
    if !is_sha256(&sha256) {
        return Err("a feed release has an invalid download sha256".into());
    }
    Ok(Release {
        version,
        channel,
        language: string(value, "language")?,
        game_language: string(game, "language")?,
        game_version: string(game, "version")?,
        min_harmonia,
        pack_hash,
        url,
        brotli,
        size: integer(download, "size")?,
        sha256,
        unpacked_size: integer(download, "unpackedSize")?,
        changelog: value
            .get("changelog")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
    })
}

/// Reads a feed document.
///
/// # Errors
/// When the document is not a version 1 feed or a field is invalid.
pub fn parse(bytes: &[u8]) -> Result<Feed, String> {
    if bytes.len() > MAX_FEED {
        return Err("the feed is too large".into());
    }
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| "the link does not lead to a feed")?;
    if root.get("format").and_then(Value::as_str) != Some("harmonia-feed") {
        return Err("the link does not lead to a feed".into());
    }
    match root.get("version").and_then(Value::as_u64) {
        Some(1) => {}
        Some(version) => return Err(format!("the feed has the unsupported version {version}")),
        None => return Err("the link does not lead to a feed".into()),
    }
    let key = optional(&root, "publisherKeyFingerprint")?;
    if key.as_deref().is_some_and(|key| !is_sha256(key)) {
        return Err("the feed has an invalid publisherKeyFingerprint".into());
    }
    let releases = root
        .get("releases")
        .and_then(Value::as_array)
        .ok_or("the feed has no releases")?;
    Ok(Feed {
        title: optional(&root, "title")?,
        key,
        releases: releases.iter().map(release).collect::<Result<_, _>>()?,
    })
}

impl Feed {
    /// The newest stable release, or testing one when `testing`. Versions
    /// have a fixed width, so they sort as text.
    #[must_use]
    pub fn newest(&self, testing: bool) -> Option<&Release> {
        self.releases
            .iter()
            .filter(|release| release.channel == "stable" || testing)
            .max_by(|a, b| a.version.cmp(&b.version))
    }
}

impl Release {
    /// Whether this Harmonia is new enough for the release.
    #[must_use]
    pub fn supported_by(&self, harmonia: &str) -> bool {
        let pad = |mut parts: Vec<u32>| {
            parts.resize(4, 0);
            parts
        };
        numbers(harmonia).is_some_and(|own| pad(own) >= pad(self.min_harmonia.clone()))
    }

    /// Whether the release should replace the installed version.
    #[must_use]
    pub fn is_newer_than(&self, installed: Option<&str>) -> bool {
        installed.is_none_or(|installed| self.version.as_str() > installed)
    }
}

/// Checks a downloaded file against its release and returns the pack's
/// bytes.
///
/// # Errors
/// When the file is not the one the feed describes.
pub fn unpack(release: &Release, file: Vec<u8>) -> Result<Vec<u8>, String> {
    const MISMATCH: &str = "the downloaded file does not match the feed";
    if file.len() as u64 != release.size
        || crate::pack::hex(&Sha256::digest(&file)) != release.sha256
    {
        return Err(MISMATCH.into());
    }
    if !release.brotli {
        return Ok(file);
    }
    if release.unpacked_size > crate::pack::MAX_PACK {
        return Err("the pack is too large".into());
    }
    let mut pack = Vec::new();
    brotli_decompressor::Decompressor::new(file.as_slice(), 64 * 1024)
        .take(release.unpacked_size + 1)
        .read_to_end(&mut pack)
        .map_err(|_| MISMATCH)?;
    if pack.len() as u64 != release.unpacked_size {
        return Err(MISMATCH.into());
    }
    Ok(pack)
}

/// Checks that the pack is the one the release names.
///
/// # Errors
/// When a fact of the pack differs from the feed.
pub fn check_pack(release: &Release, pack: &crate::pack::Pack<'_>) -> Result<(), String> {
    let manifest = &pack.manifest;
    if pack.hash_hex() != release.pack_hash
        || manifest.version != release.version
        || manifest.game.version != release.game_version
        || !manifest
            .game
            .language
            .eq_ignore_ascii_case(&release.game_language)
    {
        return Err("the downloaded pack does not match the feed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"{
      "format": "harmonia-feed", "version": 1, "title": "Sample", "unknown": [1],
      "publisherKeyFingerprint": "836b79b354f1aa84faa669fc49a0c62f23f3ade5c4d2a3d40f6d03e02afc40c7",
      "releases": [
        {"version": "2026.10.07.0001", "channel": "stable", "language": "ru",
         "game": {"language": "en", "version": "2026.09.15.0000.0000"},
         "minHarmonia": "0.1.2.0",
         "packHash": "sha256:fde694491f48a52c0a27e4cb93ef72b533955f8093946cbb0e56214e596f1c48",
         "download": {"url": "https://example.org/pack.hpk.br", "encoding": "br", "size": 5,
                      "sha256": "47e30cec8129c582f89a3f5a8613e4a350dd69f77d5e3acc9fba8411576ae1b9",
                      "unpackedSize": 9},
         "changelog": null},
        {"version": "2026.10.09.0001", "channel": "testing", "language": "ru",
         "game": {"language": "en", "version": "2026.09.15.0000.0000"},
         "minHarmonia": "9.0",
         "packHash": "sha256:fde694491f48a52c0a27e4cb93ef72b533955f8093946cbb0e56214e596f1c48",
         "download": {"url": "https://example.org/pack.hpk", "encoding": "identity", "size": 5,
                      "sha256": "47e30cec8129c582f89a3f5a8613e4a350dd69f77d5e3acc9fba8411576ae1b9",
                      "unpackedSize": 5}},
        {"version": "2026.09.30.0002", "channel": "stable", "language": "ru",
         "game": {"language": "en", "version": "2026.09.15.0000.0000"},
         "minHarmonia": "0.1",
         "packHash": "sha256:fde694491f48a52c0a27e4cb93ef72b533955f8093946cbb0e56214e596f1c48",
         "download": {"url": "https://example.org/old.hpk", "encoding": "identity", "size": 5,
                      "sha256": "47e30cec8129c582f89a3f5a8613e4a350dd69f77d5e3acc9fba8411576ae1b9",
                      "unpackedSize": 5}}
      ]}"#;

    #[test]
    fn the_newest_stable_release_is_chosen() {
        let feed = parse(FEED.as_bytes()).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Sample"));
        assert_eq!(feed.releases.len(), 3);
        let newest = feed.newest(false).unwrap();
        assert_eq!(newest.version, "2026.10.07.0001");
        assert!(newest.brotli);
        assert!(newest.supported_by("0.2.0"));
        assert!(!feed.releases[1].supported_by("0.2.0"));
        assert!(newest.is_newer_than(None));
        assert!(newest.is_newer_than(Some("2026.09.30.0002")));
        assert!(!newest.is_newer_than(Some("2026.10.07.0001")));
    }

    #[test]
    fn invalid_feeds_are_refused() {
        assert!(parse(b"<html>").is_err());
        assert!(parse(br#"{"format":"other","version":1,"releases":[]}"#).is_err());
        assert!(parse(br#"{"format":"harmonia-feed","version":2,"releases":[]}"#).is_err());
        for (from, to) in [
            (
                "https://example.org/pack.hpk.br",
                "http://example.org/pack.hpk.br",
            ),
            ("\"encoding\": \"br\"", "\"encoding\": \"gzip\""),
            ("sha256:fde6", "fde6"),
            ("\"size\": 5", "\"size\": -5"),
            ("2026.10.07.0001", "2026.13.07.0001"),
        ] {
            assert!(FEED.contains(from));
            assert!(
                parse(FEED.replacen(from, to, 1).as_bytes()).is_err(),
                "{to}"
            );
        }
    }

    #[test]
    fn links_are_https_without_spaces() {
        assert!(is_link("https://example.org/feed.json"));
        assert!(!is_link("http://example.org/feed.json"));
        assert!(!is_link("https://"));
        assert!(!is_link("https://example.org/a b"));
    }

    #[test]
    fn a_download_must_match_its_release() {
        let mut release = parse(FEED.as_bytes()).unwrap().releases.remove(1);
        let file = b"hello".to_vec();
        release.sha256 = crate::pack::hex(&Sha256::digest(&file));
        assert_eq!(unpack(&release, file.clone()).unwrap(), file);
        release.size = 4;
        assert!(unpack(&release, file.clone()).is_err());
        release.size = 5;
        release.sha256 = "0".repeat(64);
        assert!(unpack(&release, file).is_err());
    }

    #[test]
    fn a_compressed_download_unpacks_to_the_declared_size() {
        // "harmonia\n" compressed with Brotli.
        let file = vec![
            0x0b, 0x04, 0x80, 0x68, 0x61, 0x72, 0x6d, 0x6f, 0x6e, 0x69, 0x61, 0x0a, 0x03,
        ];
        let mut release = parse(FEED.as_bytes()).unwrap().releases.remove(0);
        release.size = file.len() as u64;
        release.sha256 = crate::pack::hex(&Sha256::digest(&file));
        assert_eq!(unpack(&release, file.clone()).unwrap(), b"harmonia\n");
        release.unpacked_size = 8;
        assert!(unpack(&release, file).is_err());
    }
}
