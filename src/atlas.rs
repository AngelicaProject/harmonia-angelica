//! Font atlas textures and the packing of glyphs into their free pages.

/// A game font texture (`common/font/font*.tex`): an 80-byte header, then
/// one surface of format 0x1440, 16 bits per pixel. Each 4-bit channel is a
/// glyph page: a glyph's page modulo 4 selects the channel, which lives at
/// bit 8, 4, 0, and 12 for channels 0 to 3, the order the game's glyphs use.
#[derive(Debug, Clone)]
pub struct Texture {
    bytes: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub const TEXTURE_HEADER: usize = 80;
pub const CHANNELS: u16 = 4;
const FORMAT: usize = 0x1440;
const SHIFT: [u32; 4] = [8, 4, 0, 12];

impl Texture {
    /// # Errors
    /// When the texture is not a single 0x1440 surface after its header.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < TEXTURE_HEADER {
            return Err("the texture is too small".into());
        }
        let u32_at = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
        };
        let format = u32_at(4);
        let width = usize::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        let height = usize::from(u16::from_le_bytes([bytes[10], bytes[11]]));
        let surface = u32_at(28);
        if format != FORMAT
            || width == 0
            || height == 0
            || surface != TEXTURE_HEADER
            || bytes.len() < TEXTURE_HEADER + width * height * 2
        {
            return Err("unexpected font texture format".into());
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            width,
            height,
        })
    }

    fn offset(&self, x: usize, y: usize) -> usize {
        TEXTURE_HEADER + (y * self.width + x) * 2
    }

    pub fn set(&mut self, channel: u16, x: usize, y: usize, value: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = self.offset(x, y);
        let shift = SHIFT[usize::from(channel % CHANNELS)];
        let pixel = u16::from_le_bytes([self.bytes[at], self.bytes[at + 1]]);
        let pixel = (pixel & !(0xF << shift)) | (u16::from(value & 0xF) << shift);
        self.bytes[at..at + 2].copy_from_slice(&pixel.to_le_bytes());
    }

    #[must_use]
    pub fn is_channel_empty(&self, channel: u16) -> bool {
        let mask = 0xFu16 << SHIFT[usize::from(channel % CHANNELS)];
        self.bytes[TEXTURE_HEADER..TEXTURE_HEADER + self.width * self.height * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .all(|&pixel| u16::from_le_bytes(pixel) & mask == 0)
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Where a glyph goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub page: u16,
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Clone, Copy)]
struct Shelf {
    y: usize,
    height: usize,
    cursor: usize,
}

/// Shelf packing of glyph rectangles into free pages, one pixel apart. A
/// group of rectangles is placed together or not at all; placement is
/// deterministic.
pub struct Packer {
    pages: Vec<u16>,
    width: usize,
    height: usize,
    shelves: Vec<Vec<Shelf>>,
}

const PADDING: usize = 1;

impl Packer {
    #[must_use]
    pub fn new(pages: Vec<u16>, width: usize, height: usize) -> Self {
        let shelves = vec![Vec::new(); pages.len()];
        Self {
            pages,
            width,
            height,
            shelves,
        }
    }

    /// Places every rectangle or none; taller rectangles go first. The
    /// result is in input order.
    pub fn place(&mut self, sizes: &[(usize, usize)]) -> Option<Vec<Placement>> {
        let snapshot = self.shelves.clone();
        let mut order: Vec<usize> = (0..sizes.len()).collect();
        order.sort_by(|&a, &b| {
            sizes[b]
                .1
                .cmp(&sizes[a].1)
                .then(sizes[b].0.cmp(&sizes[a].0))
                .then(a.cmp(&b))
        });
        let mut placements = vec![
            Placement {
                page: 0,
                x: 0,
                y: 0
            };
            sizes.len()
        ];
        for index in order {
            let (width, height) = sizes[index];
            let placed = if width == 0 || height == 0 {
                self.pages
                    .first()
                    .map(|&page| Placement { page, x: 0, y: 0 })
            } else {
                self.place_one(width, height)
            };
            let Some(placement) = placed else {
                self.shelves = snapshot;
                return None;
            };
            placements[index] = placement;
        }
        Some(placements)
    }

    fn place_one(&mut self, width: usize, height: usize) -> Option<Placement> {
        let (padded_width, padded_height) = (width + PADDING, height + PADDING);
        for (index, &page) in self.pages.iter().enumerate() {
            let shelves = &mut self.shelves[index];
            if let Some(shelf) = shelves.iter_mut().find(|shelf| {
                shelf.height >= padded_height && shelf.cursor + padded_width <= self.width
            }) {
                let placement = Placement {
                    page,
                    x: shelf.cursor,
                    y: shelf.y,
                };
                shelf.cursor += padded_width;
                return Some(placement);
            }
            let top = shelves.last().map_or(0, |shelf| shelf.y + shelf.height);
            if top + padded_height <= self.height && padded_width <= self.width {
                shelves.push(Shelf {
                    y: top,
                    height: padded_height,
                    cursor: padded_width,
                });
                return Some(Placement { page, x: 0, y: top });
            }
        }
        None
    }
}
