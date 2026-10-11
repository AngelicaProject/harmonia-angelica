//! A pack's glyphs in the game's own font files.
//!
//! `FONTS` glyphs are only added: a character the font has keeps its glyph.
//! Replacement glyphs take over the record of a character the font has,
//! keeping its Shift-JIS code, and are added otherwise. Kerning pairs and
//! used atlas pages never change: new bitmaps go only into candidate pages
//! that are empty in the game's texture, and a replaced glyph's old bitmap
//! stays where it was. The host checks the result against the originals
//! again before the game reads it.
use crate::{
    atlas::{CHANNELS, Packer, Texture},
    fdt::{self, Fdt},
    fonts::{Fonts, Target},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Font tables that share atlas textures. The main set is used in game; the
/// title screen loads its own `_lobby` copies with separate textures.
pub struct FontSet {
    pub name: &'static str,
    fdt_suffix: &'static str,
    texture_prefix: &'static str,
    /// Pages no game font table references; one is used only when it is
    /// also empty in the game's texture.
    candidate_pages: &'static [u16],
}

pub const SETS: [FontSet; 2] = [
    FontSet {
        name: "main",
        fdt_suffix: "",
        texture_prefix: "common/font/font",
        candidate_pages: &[10, 11, 25, 26, 27],
    },
    FontSet {
        name: "lobby",
        fdt_suffix: "_lobby",
        texture_prefix: "common/font/font_lobby",
        candidate_pages: &[23],
    },
];

impl FontSet {
    #[must_use]
    pub fn fdt_path(&self, name: &str) -> String {
        format!("common/font/{name}{}.fdt", self.fdt_suffix)
    }

    #[must_use]
    pub fn texture_path(&self, page: u16) -> String {
        format!("{}{}.tex", self.texture_prefix, page / CHANNELS + 1)
    }
}

/// What happened to one target in one set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Applied,
    AlreadyPresent,
    MissingFont,
    MetricsChanged,
    NoRoom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub set: String,
    pub target: String,
    pub status: Status,
    pub glyphs: usize,
    /// From the font-replacements section.
    pub replaced: bool,
}

/// The changed game files by game path, and a report per target.
pub struct Patched {
    pub files: BTreeMap<String, Vec<u8>>,
    pub reports: Vec<Report>,
}

/// Shift-JIS codes the game stores for these characters come from its own
/// AXIS table, which has Cyrillic.
const SHIFT_JIS_REFERENCE: &str = "AXIS_12";

/// The game's 8-bit coverage in a 4-bit channel.
#[must_use]
pub fn quantize(coverage: u8) -> u8 {
    u8::try_from((u32::from(coverage) * 15 + 127) / 255).unwrap_or(15)
}

/// Applies the sections to the game's files, which `read` returns by game
/// path (`None` for a file the game does not have).
///
/// # Errors
/// When a game font file is malformed.
pub fn patch(
    fonts: Option<&Fonts>,
    replacements: Option<&Fonts>,
    read: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Patched, String> {
    let mut patched = Patched {
        files: BTreeMap::new(),
        reports: Vec::new(),
    };
    for set in &SETS {
        patch_set(set, fonts, replacements, read, &mut patched)?;
    }
    Ok(patched)
}

struct SetState<'r> {
    read: &'r mut dyn FnMut(&str) -> Option<Vec<u8>>,
    textures: BTreeMap<String, Texture>,
    tables: BTreeMap<String, Fdt>,
}

impl SetState<'_> {
    fn texture(&mut self, path: &str) -> Result<Option<&mut Texture>, String> {
        if !self.textures.contains_key(path) {
            let Some(bytes) = (self.read)(path) else {
                return Ok(None);
            };
            self.textures.insert(
                path.into(),
                Texture::parse(&bytes).map_err(|e| format!("{path}: {e}"))?,
            );
        }
        Ok(self.textures.get_mut(path))
    }

    fn table(&mut self, path: &str) -> Result<Option<&mut Fdt>, String> {
        if !self.tables.contains_key(path) {
            let Some(bytes) = (self.read)(path) else {
                return Ok(None);
            };
            self.tables.insert(
                path.into(),
                Fdt::parse(&bytes).map_err(|e| format!("{path}: {e}"))?,
            );
        }
        Ok(self.tables.get_mut(path))
    }
}

/// What applying one target needs from its set.
struct Work<'w, 'r> {
    state: &'w mut SetState<'r>,
    packer: &'w mut Packer,
    shift_jis: &'w BTreeMap<u32, u16>,
    touched: &'w mut Vec<String>,
}

impl Work<'_, '_> {
    /// Applies one target's glyphs to its table at `path` and the set's
    /// textures; returns the outcome and the number of glyphs applied.
    fn apply(
        &mut self,
        set: &FontSet,
        section: &Fonts,
        target: &Target,
        replace: bool,
        path: &str,
    ) -> Result<(Status, usize), String> {
        let Some(table) = self.state.table(path)? else {
            return Ok((Status::MissingFont, 0));
        };
        if table.line_height() != i32::from(target.line_height)
            || table.ascent() != i32::from(target.ascent)
        {
            return Ok((Status::MetricsChanged, 0));
        }
        let glyphs: Vec<_> = target
            .glyphs
            .iter()
            .filter(|glyph| replace || table.get(fdt::pack_utf8(glyph.codepoint)).is_none())
            .collect();
        if glyphs.is_empty() {
            return Ok((Status::AlreadyPresent, 0));
        }
        let sizes: Vec<_> = glyphs
            .iter()
            .map(|g| (usize::from(g.width), usize::from(g.height)))
            .collect();
        let Some(placements) = self.packer.place(&sizes) else {
            return Ok((Status::NoRoom, 0));
        };
        for (glyph, place) in glyphs.iter().zip(&placements) {
            let texture_path = set.texture_path(place.page);
            let texture = self
                .state
                .texture(&texture_path)?
                .ok_or_else(|| format!("{texture_path} disappeared"))?;
            let bitmap = section.bitmap(glyph);
            for y in 0..usize::from(glyph.height) {
                for x in 0..usize::from(glyph.width) {
                    let coverage = quantize(bitmap[y * usize::from(glyph.width) + x]);
                    texture.set(place.page, place.x + x, place.y + y, coverage);
                }
            }
            if glyph.width > 0 && glyph.height > 0 && !self.touched.contains(&texture_path) {
                self.touched.push(texture_path);
            }
        }
        let table = self
            .state
            .table(path)?
            .ok_or_else(|| format!("{path} disappeared"))?;
        for (glyph, place) in glyphs.iter().zip(&placements) {
            let utf8 = fdt::pack_utf8(glyph.codepoint);
            let existing = table.get(utf8).copied();
            let record = fdt::Glyph {
                utf8,
                shift_jis: existing.map_or_else(
                    || self.shift_jis.get(&utf8).copied().unwrap_or(0),
                    |old| old.shift_jis,
                ),
                page: place.page,
                x: u16::try_from(place.x).unwrap_or(u16::MAX),
                y: u16::try_from(place.y).unwrap_or(u16::MAX),
                width: glyph.width,
                height: glyph.height,
                next_offset_x: i8::try_from(i16::from(glyph.advance) - i16::from(glyph.width))
                    .unwrap_or(0),
                offset_y: i8::try_from(glyph.offset_y).unwrap_or(i8::MAX),
            };
            if existing.is_some() {
                table.replace(record);
            } else {
                table.add(record);
            }
        }
        Ok((Status::Applied, glyphs.len()))
    }
}

fn patch_set(
    set: &FontSet,
    fonts: Option<&Fonts>,
    replacements: Option<&Fonts>,
    read: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    patched: &mut Patched,
) -> Result<(), String> {
    let shift_jis: BTreeMap<u32, u16> = read(&set.fdt_path(SHIFT_JIS_REFERENCE))
        .and_then(|bytes| Fdt::parse(&bytes).ok())
        .map(|table| {
            table
                .glyphs
                .iter()
                .map(|glyph| (glyph.utf8, glyph.shift_jis))
                .collect()
        })
        .unwrap_or_default();
    let mut state = SetState {
        read,
        textures: BTreeMap::new(),
        tables: BTreeMap::new(),
    };
    let mut free = Vec::new();
    let mut size = (0, 0);
    for &page in set.candidate_pages {
        if let Some(texture) = state.texture(&set.texture_path(page))?
            && texture.is_channel_empty(page % CHANNELS)
        {
            if free.is_empty() {
                size = (texture.width, texture.height);
            }
            free.push(page);
        }
    }
    let mut packer = Packer::new(free, size.0, size.1);
    let mut changed_tables: Vec<String> = Vec::new();
    let mut touched: Vec<String> = Vec::new();

    for (section, replace) in [(fonts, false), (replacements, true)] {
        let Some(section) = section else { continue };
        for target in &section.targets {
            let path = set.fdt_path(&target.name());
            let mut work = Work {
                state: &mut state,
                packer: &mut packer,
                shift_jis: &shift_jis,
                touched: &mut touched,
            };
            let (status, glyphs) = work.apply(set, section, target, replace, &path)?;
            if status == Status::Applied && !changed_tables.contains(&path) {
                changed_tables.push(path);
            }
            patched.reports.push(Report {
                set: set.name.into(),
                target: target.name(),
                status,
                glyphs,
                replaced: replace,
            });
        }
    }
    for path in changed_tables {
        patched
            .files
            .insert(path.clone(), state.tables[&path].write());
    }
    for path in touched {
        patched
            .files
            .insert(path.clone(), state.textures[&path].bytes().to_vec());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{atlas::TEXTURE_HEADER, pack::Pack};

    const FONTS: &[u8] = include_bytes!("../tests/data/harmonia-interop-fonts.hpk");
    const REPLACEMENTS: &[u8] = include_bytes!("../tests/data/harmonia-interop-replacements.hpk");

    /// A font table with ASCII 'A' on page 0 and the given metrics.
    fn table(line_height: i32, ascent: i32, extra: &[u32]) -> Vec<u8> {
        let mut utf8: Vec<u32> = [0x41].iter().chain(extra).copied().collect();
        utf8.sort_unstable();
        let knhd = 0x40 + utf8.len() * fdt::GLYPH;
        let mut bytes = b"fcsv0100".to_vec();
        bytes.extend_from_slice(&0x20u32.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(knhd).unwrap().to_le_bytes());
        bytes.resize(0x20, 0);
        bytes.extend_from_slice(b"fthd");
        bytes.extend_from_slice(&u32::try_from(utf8.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&256u16.to_le_bytes());
        bytes.extend_from_slice(&256u16.to_le_bytes());
        bytes.extend_from_slice(&12f32.to_le_bytes());
        bytes.extend_from_slice(&line_height.to_le_bytes());
        bytes.extend_from_slice(&ascent.to_le_bytes());
        for code in utf8 {
            let mut record = [0u8; fdt::GLYPH];
            record[..4].copy_from_slice(&code.to_le_bytes());
            record[12] = 4;
            record[13] = 4;
            bytes.extend_from_slice(&record);
        }
        bytes.extend_from_slice(b"knhd");
        bytes.extend_from_slice(&[0; 12]);
        bytes
    }

    /// An empty 256×256 font texture.
    fn texture() -> Vec<u8> {
        let mut bytes = vec![0u8; TEXTURE_HEADER + 256 * 256 * 2];
        bytes[4..8].copy_from_slice(&0x1440u32.to_le_bytes());
        bytes[8..10].copy_from_slice(&256u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&256u16.to_le_bytes());
        bytes[28..32].copy_from_slice(&80u32.to_le_bytes());
        bytes
    }

    fn game(pack: &Pack<'_>) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        let sections = [pack.fonts.as_ref(), pack.font_replacements.as_ref()];
        for set in &SETS {
            for target in sections.iter().flatten().flat_map(|fonts| &fonts.targets) {
                let first = target.glyphs[0].codepoint;
                let existing = [fdt::pack_utf8(first)];
                files.insert(
                    set.fdt_path(&target.name()),
                    table(
                        i32::from(target.line_height),
                        i32::from(target.ascent),
                        &existing,
                    ),
                );
            }
            for texture_number in 1..=7 {
                files.insert(
                    format!("{}{texture_number}.tex", set.texture_prefix),
                    texture(),
                );
            }
        }
        files
    }

    #[test]
    fn glyphs_go_into_free_pages_and_pass_the_hosts_checks() {
        for bytes in [FONTS, REPLACEMENTS] {
            let pack = Pack::parse(bytes).unwrap();
            let originals = game(&pack);
            let mut read = |path: &str| originals.get(path).cloned();
            let patched = patch(
                pack.fonts.as_ref(),
                pack.font_replacements.as_ref(),
                &mut read,
            )
            .unwrap();
            assert!(
                patched.reports.iter().any(|r| r.status == Status::Applied),
                "{:?}",
                patched.reports
            );
            assert!(!patched.files.is_empty());
            for (path, contents) in &patched.files {
                runtime::file_formats::check(path, &originals[path], contents)
                    .unwrap_or_else(|e| panic!("{path}: {e}"));
            }
            for (path, contents) in patched.files.iter().filter(|(p, _)| {
                std::path::Path::new(p)
                    .extension()
                    .is_some_and(|e| e == "fdt")
            }) {
                let table = Fdt::parse(contents).unwrap();
                let original = Fdt::parse(&originals[path]).unwrap();
                assert!(table.glyphs.len() >= original.glyphs.len(), "{path}");
                let changed = table
                    .glyphs
                    .iter()
                    .filter(|g| original.get(g.utf8) != Some(*g) && g.width > 0);
                for glyph in changed {
                    assert!(
                        [10, 11, 25, 26, 27, 23].contains(&glyph.page),
                        "{path}: page {}",
                        glyph.page
                    );
                }
            }
        }
    }

    #[test]
    fn missing_fonts_and_changed_metrics_are_reported() {
        let pack = Pack::parse(FONTS).unwrap();
        let mut originals = game(&pack);
        let target = &pack.fonts.as_ref().unwrap().targets[0];
        originals.insert(SETS[0].fdt_path(&target.name()), table(99, 1, &[]));
        originals.remove(&SETS[1].fdt_path(&target.name()));
        let mut read = |path: &str| originals.get(path).cloned();
        let patched = patch(pack.fonts.as_ref(), None, &mut read).unwrap();
        let status = |set: &str| {
            patched
                .reports
                .iter()
                .find(|r| r.set == set && r.target == target.name())
                .unwrap()
                .status
        };
        assert_eq!(status("main"), Status::MetricsChanged);
        assert_eq!(status("lobby"), Status::MissingFont);
        assert_eq!(quantize(255), 15);
        assert_eq!(quantize(0), 0);
    }

    /// A real pack against the installed game: `HARMONIA_PACK` names the
    /// pack, `ANGELICA_GAME_PATH` the game's `game` folder.
    #[test]
    #[ignore = "needs HARMONIA_PACK and ANGELICA_GAME_PATH"]
    fn a_real_pack_patches_the_installed_games_fonts() {
        let bytes = std::fs::read(std::env::var("HARMONIA_PACK").unwrap()).unwrap();
        let pack = Pack::parse(&bytes).unwrap();
        let game = std::path::PathBuf::from(std::env::var("ANGELICA_GAME_PATH").unwrap());
        let data = angelica_sqpack::GameData::open(game.parent().unwrap()).unwrap();
        let mut read = |path: &str| data.file(path).unwrap();
        let patched = patch(
            pack.fonts.as_ref(),
            pack.font_replacements.as_ref(),
            &mut read,
        )
        .unwrap();
        for report in &patched.reports {
            eprintln!("{report:?}");
        }
        for (path, contents) in &patched.files {
            let original = data.file(path).unwrap().unwrap();
            runtime::file_formats::check(path, &original, contents)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            eprintln!("{path}: {} bytes", contents.len());
        }
        assert!(patched.reports.iter().any(|r| r.status == Status::Applied));
    }
}
