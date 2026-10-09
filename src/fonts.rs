//! The `FONTS` section and the font-replacements section of a pack (Aeria
//! `docs/formats/pack-v1.md`, format minors 1 and 2): glyphs for the game's
//! fonts. Parsing checks every rule of the section.
use crate::pack::Error;
use serde::Deserialize;

const MAGIC: &[u8; 8] = b"HPKFONT1";
const HEADER: usize = 32;
const GLYPH_RECORD: usize = 16;

/// A font a section's glyphs were rendered from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Source {
    pub family: String,
    pub copyright: String,
    pub license: String,
    pub license_text: String,
    pub sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawTarget {
    font: String,
    size: String,
    line_height: u32,
    ascent: u32,
    source: usize,
    glyph_start: usize,
    glyph_count: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    sources: Vec<Source>,
    targets: Vec<RawTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub codepoint: u32,
    pub width: u8,
    pub height: u8,
    pub offset_y: u8,
    pub advance: u8,
    bitmap: usize,
}

/// Glyphs for one game font, `common/font/<font>_<size>.fdt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub font: String,
    pub size: String,
    pub line_height: u8,
    pub ascent: u8,
    pub source: usize,
    pub glyphs: Vec<Glyph>,
}

impl Target {
    /// The font table's name without the `_lobby` suffix.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{}_{}", self.font, self.size)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fonts {
    pub sources: Vec<Source>,
    pub targets: Vec<Target>,
    bitmaps: Vec<u8>,
}

fn fail<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error(message.into()))
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize
}

impl Fonts {
    /// The coverage bitmap of a glyph, `width × height` bytes, row-major.
    #[must_use]
    pub fn bitmap(&self, glyph: &Glyph) -> &[u8] {
        &self.bitmaps
            [glyph.bitmap..glyph.bitmap + usize::from(glyph.width) * usize::from(glyph.height)]
    }

    /// # Errors
    /// Names the first rule of the section the bytes break.
    pub fn parse(section: &[u8]) -> Result<Self, Error> {
        if section.len() < HEADER
            || &section[..8] != MAGIC
            || section[20..HEADER].iter().any(|&b| b != 0)
        {
            return fail("invalid FONTS header");
        }
        let (metadata_length, glyph_count, bitmap_length) =
            (u32_at(section, 8), u32_at(section, 12), u32_at(section, 16));
        let metadata_end = HEADER + metadata_length;
        let glyphs_start = (metadata_end + 7) & !7;
        let bitmaps_start = glyphs_start + glyph_count * GLYPH_RECORD;
        if bitmaps_start + bitmap_length != section.len()
            || section[metadata_end..glyphs_start].iter().any(|&b| b != 0)
        {
            return fail("FONTS section lengths do not match its header");
        }
        let metadata: Metadata = serde_json::from_slice(&section[HEADER..metadata_end])
            .map_err(|e| Error(format!("FONTS metadata is invalid: {e}")))?;
        if metadata.targets.is_empty() {
            return fail("FONTS metadata needs at least one target");
        }
        for source in &metadata.sources {
            let present = |text: &str| !text.trim().is_empty();
            if !present(&source.family)
                || !present(&source.copyright)
                || !present(&source.license)
                || source.license_text.contains('\r')
                || source.sha256.len() != 64
                || !source
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return fail("invalid FONTS source");
            }
        }

        let mut used = vec![false; metadata.sources.len()];
        let mut targets: Vec<Target> = Vec::with_capacity(metadata.targets.len());
        let (mut next_glyph, mut next_bitmap) = (0usize, 0usize);
        for raw in metadata.targets {
            let name = format!("{}_{}", raw.font, raw.size);
            if raw.font.is_empty()
                || !raw.font.bytes().all(|b| b.is_ascii_alphanumeric())
                || raw.size.is_empty()
                || !raw.size.bytes().all(|b| b.is_ascii_digit())
                || !(1..=255).contains(&raw.line_height)
                || raw.ascent < 1
                || raw.ascent > raw.line_height
                || raw.source >= metadata.sources.len()
                || raw.glyph_count < 1
                || raw.glyph_start != next_glyph
                || raw.glyph_start + raw.glyph_count > glyph_count
            {
                return fail(format!("invalid FONTS target {name}"));
            }
            if let Some(previous) = targets.last()
                && (previous.font.as_str(), previous.size.as_str())
                    >= (raw.font.as_str(), raw.size.as_str())
            {
                return fail("FONTS targets are not sorted and unique");
            }
            used[raw.source] = true;
            let (Ok(line_height), Ok(ascent)) =
                (u8::try_from(raw.line_height), u8::try_from(raw.ascent))
            else {
                return fail(format!("invalid FONTS target {name}"));
            };
            let glyphs = read_glyphs(
                section,
                glyphs_start,
                &raw,
                line_height,
                &mut next_bitmap,
                &name,
            )?;
            if next_bitmap > bitmap_length {
                return fail("FONTS bitmaps are out of range");
            }
            next_glyph += raw.glyph_count;
            targets.push(Target {
                font: raw.font,
                size: raw.size,
                line_height,
                ascent,
                source: raw.source,
                glyphs,
            });
        }
        if next_glyph != glyph_count || next_bitmap != bitmap_length || used.contains(&false) {
            return fail("FONTS section has unreferenced glyphs, bitmaps, or sources");
        }
        Ok(Self {
            sources: metadata.sources,
            targets,
            bitmaps: section[bitmaps_start..].to_vec(),
        })
    }
}

/// The glyph records of one target, checked in order; `next_bitmap` is the
/// offset the first glyph's bitmap must start at, and the end of the last
/// one afterwards.
fn read_glyphs(
    section: &[u8],
    glyphs_start: usize,
    raw: &RawTarget,
    line_height: u8,
    next_bitmap: &mut usize,
    name: &str,
) -> Result<Vec<Glyph>, Error> {
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(raw.glyph_count);
    for index in raw.glyph_start..raw.glyph_start + raw.glyph_count {
        let record = &section[glyphs_start + index * GLYPH_RECORD..][..GLYPH_RECORD];
        let glyph = Glyph {
            codepoint: u32::try_from(u32_at(record, 0)).unwrap_or(u32::MAX),
            width: record[4],
            height: record[5],
            offset_y: record[6],
            advance: record[7],
            bitmap: u32_at(record, 8),
        };
        let advance_delta = i16::from(glyph.advance) - i16::from(glyph.width);
        if glyph.codepoint < 0x80
            || char::from_u32(glyph.codepoint).is_none()
            || glyphs
                .last()
                .is_some_and(|last| last.codepoint >= glyph.codepoint)
            || u16::from(glyph.offset_y) + u16::from(glyph.height) > u16::from(line_height)
            || !(-128..=127).contains(&advance_delta)
            || glyph.bitmap != *next_bitmap
            || u32_at(record, 12) != 0
        {
            return fail(format!(
                "invalid glyph U+{:04X} in FONTS target {name}",
                glyph.codepoint
            ));
        }
        *next_bitmap += usize::from(glyph.width) * usize::from(glyph.height);
        glyphs.push(glyph);
    }
    Ok(glyphs)
}
