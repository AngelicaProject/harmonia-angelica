//! A game font table (`common/font/*.fdt`):
//!
//! ```text
//! 0x00  "fcsv0100", u32 fthd offset, u32 knhd offset, zeros to 0x20
//! fthd  "fthd", u32 glyph count, u32 kerning count, u32 padding,
//!       u16 texture width, u16 texture height, f32 size, i32 line height, i32 ascent
//!       glyph count × 16-byte records sorted by their packed UTF-8
//! knhd  "knhd", u32 count, 8 bytes padding, count × 16-byte kerning pairs
//! ```
//!
//! Only glyph records are ever added or replaced; everything else is
//! written back as read.

const HEADER: usize = 0x20;
const FONT_HEADER: usize = 0x20;
const KERNING_HEADER: usize = 0x10;
const KERNING_PAIR: usize = 16;
pub const GLYPH: usize = 16;

/// A glyph record. `utf8` is the character's UTF-8 bytes packed big-endian
/// into a `u32` (`A` = 0x41, `А` = 0xD090), so record order is codepoint
/// order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub utf8: u32,
    pub shift_jis: u16,
    pub page: u16,
    pub x: u16,
    pub y: u16,
    pub width: u8,
    pub height: u8,
    pub next_offset_x: i8,
    pub offset_y: i8,
}

#[derive(Debug, Clone)]
pub struct Fdt {
    header: Vec<u8>,
    font_header: Vec<u8>,
    pub glyphs: Vec<Glyph>,
    kerning: Vec<u8>,
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize
}
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

/// The packed UTF-8 of a character.
#[must_use]
pub fn pack_utf8(codepoint: u32) -> u32 {
    let mut buffer = [0u8; 4];
    char::from_u32(codepoint).map_or(0, |c| {
        c.encode_utf8(&mut buffer)
            .bytes()
            .fold(0u32, |packed, byte| (packed << 8) | u32::from(byte))
    })
}

impl Fdt {
    /// # Errors
    /// Names the layout rule the bytes break.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < HEADER + FONT_HEADER + KERNING_HEADER || &bytes[..8] != b"fcsv0100" {
            return Err("not an fcsv0100 font table".into());
        }
        let (fthd, knhd) = (u32_at(bytes, 8), u32_at(bytes, 12));
        if fthd != HEADER || &bytes[fthd..fthd + 4] != b"fthd" {
            return Err("unexpected font header".into());
        }
        let count = u32_at(bytes, fthd + 4);
        let kerning = u32_at(bytes, fthd + 8);
        if count > 0x10000
            || knhd != fthd + FONT_HEADER + count * GLYPH
            || knhd + KERNING_HEADER > bytes.len()
            || &bytes[knhd..knhd + 4] != b"knhd"
        {
            return Err("unexpected kerning header".into());
        }
        if u32_at(bytes, knhd + 4) != kerning
            || bytes.len() != knhd + KERNING_HEADER + kerning * KERNING_PAIR
        {
            return Err("the kerning table does not match its header".into());
        }
        let mut glyphs: Vec<Glyph> = Vec::with_capacity(count);
        for index in 0..count {
            let at = &bytes[fthd + FONT_HEADER + index * GLYPH..][..GLYPH];
            let glyph = Glyph {
                utf8: u32::try_from(u32_at(at, 0)).unwrap_or(u32::MAX),
                shift_jis: u16_at(at, 4),
                page: u16_at(at, 6),
                x: u16_at(at, 8),
                y: u16_at(at, 10),
                width: at[12],
                height: at[13],
                next_offset_x: i8::from_le_bytes([at[14]]),
                offset_y: i8::from_le_bytes([at[15]]),
            };
            // The game's AXIS tables hold the space twice; equal neighbours
            // are kept and written back as read.
            if glyphs.last().is_some_and(|last| last.utf8 > glyph.utf8) {
                return Err("glyph records are not sorted".into());
            }
            glyphs.push(glyph);
        }
        Ok(Self {
            header: bytes[..HEADER].to_vec(),
            font_header: bytes[fthd..fthd + FONT_HEADER].to_vec(),
            glyphs,
            kerning: bytes[knhd..].to_vec(),
        })
    }

    #[must_use]
    pub fn line_height(&self) -> i32 {
        let h = &self.font_header;
        i32::from_le_bytes([h[0x18], h[0x19], h[0x1A], h[0x1B]])
    }

    #[must_use]
    pub fn ascent(&self) -> i32 {
        let h = &self.font_header;
        i32::from_le_bytes([h[0x1C], h[0x1D], h[0x1E], h[0x1F]])
    }

    fn find(&self, utf8: u32) -> Result<usize, usize> {
        self.glyphs.binary_search_by(|glyph| glyph.utf8.cmp(&utf8))
    }

    #[must_use]
    pub fn get(&self, utf8: u32) -> Option<&Glyph> {
        self.find(utf8).ok().map(|index| &self.glyphs[index])
    }

    /// Inserts a glyph the font does not have, in order.
    pub fn add(&mut self, glyph: Glyph) {
        if let Err(index) = self.find(glyph.utf8) {
            self.glyphs.insert(index, glyph);
        }
    }

    /// Replaces the record of a glyph the font has.
    pub fn replace(&mut self, glyph: Glyph) {
        if let Ok(index) = self.find(glyph.utf8) {
            self.glyphs[index] = glyph;
        }
    }

    #[must_use]
    pub fn write(&self) -> Vec<u8> {
        let knhd = HEADER + FONT_HEADER + self.glyphs.len() * GLYPH;
        let mut bytes = Vec::with_capacity(knhd + self.kerning.len());
        bytes.extend_from_slice(&self.header);
        bytes[12..16].copy_from_slice(&u32::try_from(knhd).unwrap_or(u32::MAX).to_le_bytes());
        bytes.extend_from_slice(&self.font_header);
        bytes[HEADER + 4..HEADER + 8].copy_from_slice(
            &u32::try_from(self.glyphs.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for glyph in &self.glyphs {
            bytes.extend_from_slice(&glyph.utf8.to_le_bytes());
            bytes.extend_from_slice(&glyph.shift_jis.to_le_bytes());
            bytes.extend_from_slice(&glyph.page.to_le_bytes());
            bytes.extend_from_slice(&glyph.x.to_le_bytes());
            bytes.extend_from_slice(&glyph.y.to_le_bytes());
            bytes.push(glyph.width);
            bytes.push(glyph.height);
            bytes.extend_from_slice(&glyph.next_offset_x.to_le_bytes());
            bytes.extend_from_slice(&glyph.offset_y.to_le_bytes());
        }
        bytes.extend_from_slice(&self.kerning);
        bytes
    }
}
