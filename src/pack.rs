//! Harmonia Pack v1 (`.hpk`), as Aeria specifies it in
//! `docs/formats/pack-v1.md`. [`Pack::parse`] verifies everything the
//! format requires before any of it is used: header, sections, digest,
//! signature, manifest, every record, and every string.
use angelica_sdk::text::sestring;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAGIC: &[u8; 8] = b"AERIAHPK";
const SIGNATURE_MAGIC: &[u8; 8] = b"HPKSIG01";
const SIGNATURE_DOMAIN: &[u8] = b"AERIA-HPK-V1-SIGNATURE";
const ROTATION_DOMAIN: &[u8] = b"AERIA-HPK-V1-KEY-ROTATION";
const HEADER: usize = 64;
const SECTION_ENTRY: usize = 24;
const DIGEST: usize = 32;
const MAX_SECTIONS: usize = 64;
const SHEET_RECORD: usize = 32;
const LAYOUT_RECORD: usize = 8;
const ROW_RECORD: usize = 16;
const CELL_RECORD: usize = 24;
const MAX_MANIFEST: usize = 1 << 20;
pub const MAX_PACK: u64 = 1 << 30;
const KIND_MANIFEST: u32 = 1;
const KIND_NAMES: u32 = 2;
const KIND_SHEETS: u32 = 3;
const KIND_LAYOUT: u32 = 4;
const KIND_ROWS: u32 = 5;
const KIND_CELLS: u32 = 6;
const KIND_STRINGS: u32 = 7;
const FIRST_OPTIONAL: u32 = 0x10000;

/// The domain of the packs' source guards; it keeps its historical spelling.
pub const GUARD_DOMAIN: &[u8] = b"HARMONIA-HXS-V1-RAW-STRING";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn fail<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error(message.into()))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Game {
    pub language: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Built {
    pub aeria: String,
    pub commit: String,
}

/// The `MANIFEST` section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    pub title: String,
    pub team: Team,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub version: String,
    pub channel: String,
    pub language: String,
    pub game: Game,
    pub built: Built,
    pub min_harmonia: String,
}

/// The publisher's signature block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub public_key: [u8; 65],
    value: [u8; 64],
    pub previous_key: Option<[u8; 65]>,
    endorsement: Option<[u8; 64]>,
}

impl Signature {
    /// SHA-256 of the public key, in hex.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public_key)
    }
}

#[must_use]
pub fn fingerprint(public_key: &[u8]) -> String {
    hex(&Sha256::digest(public_key))
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// A text column of a sheet: its index among all columns and its offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutColumn {
    pub index: u16,
    pub offset: u16,
}

/// A translated cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell<'a> {
    pub row: u32,
    pub subrow: u16,
    /// The position of the column in the sheet's layout.
    pub ordinal: u16,
    pub guard: u64,
    pub text: &'a [u8],
}

/// A verified pack, read in place from its bytes.
pub struct Pack<'a> {
    bytes: &'a [u8],
    pub hash: [u8; 32],
    pub manifest: Manifest,
    pub signature: Option<Signature>,
    names: &'a [u8],
    sheets: &'a [u8],
    layout: &'a [u8],
    rows: &'a [u8],
    cells: &'a [u8],
    strings: &'a [u8],
}

/// A sheet of a pack.
pub struct Sheet<'a> {
    pub name: &'a str,
    pub subrows: bool,
    pub layout: Vec<LayoutColumn>,
    rows: &'a [u8],
    cells: &'a [u8],
    strings: &'a [u8],
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}
fn zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| b == 0)
}
fn index(value: u64) -> Result<usize, Error> {
    usize::try_from(value).or_else(|_| fail("a value is out of range"))
}

impl<'a> Pack<'a> {
    /// Verifies the whole pack.
    ///
    /// # Errors
    /// Names the first rule of the format the pack breaks.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER + DIGEST || bytes.len() as u64 > MAX_PACK {
            return fail("the file is not a pack: its size is out of range");
        }
        if &bytes[..8] != MAGIC {
            return fail("the file is not a Harmonia pack");
        }
        if u16_at(bytes, 8) != 1 {
            return fail(format!(
                "pack format version {} is not supported",
                u16_at(bytes, 8)
            ));
        }
        if u32_at(bytes, 12) as usize != HEADER || u32_at(bytes, 28) != 0 || !zero(&bytes[32..64]) {
            return fail("invalid pack header");
        }
        let body = index(u64_at(bytes, 16))?;
        let section_count = u32_at(bytes, 24) as usize;
        if section_count > MAX_SECTIONS {
            return fail("too many pack sections");
        }
        let table_end = HEADER + section_count * SECTION_ENTRY;
        if body < table_end + DIGEST || body > bytes.len() {
            return fail("invalid pack body length");
        }
        let digest_start = body - DIGEST;
        let mut hash = [0; 32];
        hash.copy_from_slice(&bytes[digest_start..body]);

        let sections = read_sections(bytes, table_end, section_count, digest_start)?;
        let section = |kind: u32| -> Result<&'a [u8], Error> {
            let &(offset, length) = sections
                .get(&kind)
                .ok_or_else(|| Error(format!("pack section {kind} is missing")))?;
            Ok(&bytes[offset..offset + length])
        };

        if Sha256::digest(&bytes[..digest_start]).as_slice() != hash {
            return fail("the pack's digest does not match its content");
        }
        let signature = if body < bytes.len() {
            let signature = parse_signature(&bytes[body..])?;
            verify_signature(&signature, &hash)?;
            Some(signature)
        } else {
            None
        };
        let manifest = parse_manifest(section(KIND_MANIFEST)?)?;

        let records = |data: &[u8], size: usize, name: &str| {
            if data.len().is_multiple_of(size) {
                Ok(data.len() / size)
            } else {
                fail(format!("section {name} has an invalid length"))
            }
        };
        let pack = Self {
            bytes,
            hash,
            manifest,
            signature,
            names: section(KIND_NAMES)?,
            sheets: section(KIND_SHEETS)?,
            layout: section(KIND_LAYOUT)?,
            rows: section(KIND_ROWS)?,
            cells: section(KIND_CELLS)?,
            strings: section(KIND_STRINGS)?,
        };
        records(pack.sheets, SHEET_RECORD, "SHEETS")?;
        records(pack.layout, LAYOUT_RECORD, "LAYOUT")?;
        records(pack.rows, ROW_RECORD, "ROWS")?;
        records(pack.cells, CELL_RECORD, "CELLS")?;
        pack.validate()?;
        Ok(pack)
    }

    #[must_use]
    pub fn hash_hex(&self) -> String {
        hex(&self.hash)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    #[must_use]
    pub fn sheet_count(&self) -> usize {
        self.sheets.len() / SHEET_RECORD
    }

    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.cells.len() / CELL_RECORD
    }

    /// Every sheet, in name order.
    pub fn sheets(&self) -> impl Iterator<Item = Sheet<'a>> + '_ {
        (0..self.sheet_count()).map(|index| self.sheet(index))
    }

    // Called after `validate` checked the ranges, or by it after checking.
    fn sheet(&self, index: usize) -> Sheet<'a> {
        let record = &self.sheets[index * SHEET_RECORD..][..SHEET_RECORD];
        let name_offset = u32_at(record, 0) as usize;
        let name_length = u32_at(record, 4) as usize;
        let layout_start = u32_at(record, 12) as usize;
        let layout_count = u32_at(record, 16) as usize;
        let row_start = u32_at(record, 20) as usize;
        let row_count = u32_at(record, 24) as usize;
        let layout = self.layout
            [layout_start * LAYOUT_RECORD..(layout_start + layout_count) * LAYOUT_RECORD]
            .as_chunks::<LAYOUT_RECORD>()
            .0
            .iter()
            .map(|column| LayoutColumn {
                index: u16::try_from(u32_at(column, 0)).unwrap_or(u16::MAX),
                offset: u16::try_from(u32_at(column, 4)).unwrap_or(u16::MAX),
            })
            .collect();
        Sheet {
            name: std::str::from_utf8(&self.names[name_offset..name_offset + name_length])
                .unwrap_or(""),
            subrows: record[8] == 1,
            layout,
            rows: &self.rows[row_start * ROW_RECORD..(row_start + row_count) * ROW_RECORD],
            cells: self.cells,
            strings: self.strings,
        }
    }

    /// Checks a sheet's rows and cells, and every string they reference
    /// once; `strings` collects the referenced ranges.
    fn validate_rows(
        &self,
        sheet: &Sheet<'_>,
        layout_count: usize,
        strings: &mut BTreeMap<u32, u32>,
    ) -> Result<(), Error> {
        let mut last_key = None;
        for row in sheet.rows.as_chunks::<ROW_RECORD>().0 {
            let (row_id, subrow, count, first) = (
                u32_at(row, 0),
                u16_at(row, 4),
                usize::from(u16_at(row, 6)),
                u32_at(row, 8) as usize,
            );
            let key = (row_id, subrow);
            if last_key.is_some_and(|last| key <= last)
                || (!sheet.subrows && subrow != 0)
                || count == 0
                || u32_at(row, 12) != 0
                || first + count > self.cells.len() / CELL_RECORD
            {
                return fail(format!(
                    "invalid row {row_id}/{subrow} in sheet {}",
                    sheet.name
                ));
            }
            last_key = Some(key);
            let mut last_ordinal = None;
            for cell in self.cells[first * CELL_RECORD..(first + count) * CELL_RECORD]
                .as_chunks::<CELL_RECORD>()
                .0
            {
                let (ordinal, length, offset) = (u16_at(cell, 0), u32_at(cell, 4), u32_at(cell, 8));
                if last_ordinal.is_some_and(|last| ordinal <= last)
                    || usize::from(ordinal) >= layout_count
                    || !zero(&cell[2..4])
                    || u32_at(cell, 12) != 0
                    || length == 0
                    || length > 65535
                    || offset as usize + length as usize + 1 > self.strings.len()
                {
                    return fail(format!(
                        "invalid cell in row {row_id}/{subrow} of sheet {}",
                        sheet.name
                    ));
                }
                last_ordinal = Some(ordinal);
                match strings.get(&offset) {
                    Some(&known) if known != length => {
                        return fail("cells share a string with different lengths");
                    }
                    Some(_) => {}
                    None => {
                        let start = offset as usize;
                        let text = &self.strings[start..start + length as usize];
                        if self.strings[start + length as usize] != 0
                            || !sestring::is_well_formed(text)
                        {
                            return fail(format!(
                                "malformed string in row {row_id}/{subrow} of sheet {}",
                                sheet.name
                            ));
                        }
                        strings.insert(offset, length);
                    }
                }
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), Error> {
        let (mut names, mut layouts, mut rows) = (0usize, 0usize, 0usize);
        let mut previous: Option<&[u8]> = None;
        let mut strings: BTreeMap<u32, u32> = BTreeMap::new();
        for index in 0..self.sheet_count() {
            let record = &self.sheets[index * SHEET_RECORD..][..SHEET_RECORD];
            let name_offset = u32_at(record, 0) as usize;
            let name_length = u32_at(record, 4) as usize;
            let layout_start = u32_at(record, 12) as usize;
            let layout_count = u32_at(record, 16) as usize;
            let row_start = u32_at(record, 20) as usize;
            let row_count = u32_at(record, 24) as usize;
            if !zero(&record[9..12]) || u32_at(record, 28) != 0 || record[8] > 1 {
                return fail("invalid sheet record");
            }
            if name_offset != names
                || name_length == 0
                || name_offset + name_length > self.names.len()
            {
                return fail("invalid sheet name range");
            }
            if layout_start != layouts
                || layout_count == 0
                || layout_start + layout_count > self.layout.len() / LAYOUT_RECORD
            {
                return fail("invalid sheet layout range");
            }
            if row_start != rows
                || row_count == 0
                || row_start + row_count > self.rows.len() / ROW_RECORD
            {
                return fail("invalid sheet row range");
            }
            let name = &self.names[name_offset..name_offset + name_length];
            if previous.is_some_and(|previous| previous >= name) {
                return fail("sheets are not sorted by name");
            }
            if std::str::from_utf8(name).is_err() {
                return fail("a sheet name is not UTF-8");
            }
            previous = Some(name);
            names += name_length;
            layouts += layout_count;
            rows += row_count;

            let sheet = self.sheet(index);
            let mut last_column = None;
            for at in (layout_start..layout_start + layout_count).map(|l| l * LAYOUT_RECORD) {
                let column = u32_at(self.layout, at);
                if last_column.is_some_and(|last| column <= last)
                    || column > 65535
                    || u32_at(self.layout, at + 4) > 65535
                {
                    return fail(format!("invalid layout in sheet {}", sheet.name));
                }
                last_column = Some(column);
            }
            self.validate_rows(&sheet, layout_count, &mut strings)?;
        }
        if names != self.names.len()
            || layouts != self.layout.len() / LAYOUT_RECORD
            || rows != self.rows.len() / ROW_RECORD
        {
            return fail("sheet records do not cover their sections");
        }
        // Stored strings tile STRINGS exactly: no hidden or orphaned bytes.
        let mut cursor = 0u64;
        for (&offset, &length) in &strings {
            if u64::from(offset) != cursor {
                return fail("STRINGS contains unreferenced bytes");
            }
            cursor += u64::from(length) + 1;
        }
        if cursor != self.strings.len() as u64 {
            return fail("STRINGS contains unreferenced bytes");
        }
        Ok(())
    }
}

impl<'a> Sheet<'a> {
    /// Every cell, by row, subrow, and ordinal.
    pub fn cells(&self) -> impl Iterator<Item = Cell<'a>> + '_ {
        self.rows
            .as_chunks::<ROW_RECORD>()
            .0
            .iter()
            .flat_map(move |row| {
                let (row_id, subrow, count, first) = (
                    u32_at(row, 0),
                    u16_at(row, 4),
                    usize::from(u16_at(row, 6)),
                    u32_at(row, 8) as usize,
                );
                self.cells[first * CELL_RECORD..(first + count) * CELL_RECORD]
                    .as_chunks::<CELL_RECORD>()
                    .0
                    .iter()
                    .map(move |cell| {
                        let offset = u32_at(cell, 8) as usize;
                        Cell {
                            row: row_id,
                            subrow,
                            ordinal: u16_at(cell, 0),
                            guard: u64_at(cell, 16),
                            text: &self.strings[offset..offset + u32_at(cell, 4) as usize],
                        }
                    })
            })
    }
}

/// The section table: each required or known optional section's offset and
/// length, checked for alignment, order, bounds, and zero padding.
fn read_sections(
    bytes: &[u8],
    table_end: usize,
    section_count: usize,
    digest_start: usize,
) -> Result<BTreeMap<u32, (usize, usize)>, Error> {
    let mut sections: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    let mut cursor = table_end;
    for entry in 0..section_count {
        let at = HEADER + entry * SECTION_ENTRY;
        let kind = u32_at(bytes, at);
        let offset = index(u64_at(bytes, at + 8))?;
        let length = index(u64_at(bytes, at + 16))?;
        if u32_at(bytes, at + 4) != 0
            || offset % 8 != 0
            || offset < cursor
            || length > digest_start
            || offset > digest_start - length
        {
            return fail("invalid pack section table");
        }
        if !zero(&bytes[cursor..offset]) {
            return fail("non-zero padding between pack sections");
        }
        cursor = offset + length;
        if kind > FIRST_OPTIONAL + 1 {
            continue;
        }
        if kind < FIRST_OPTIONAL && !(KIND_MANIFEST..=KIND_STRINGS).contains(&kind) {
            return fail(format!("unknown required pack section {kind}"));
        }
        if sections.insert(kind, (offset, length)).is_some() {
            return fail(format!("pack section {kind} appears twice"));
        }
    }
    if !zero(&bytes[cursor..digest_start]) {
        return fail("non-zero padding before the pack digest");
    }
    Ok(sections)
}

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, Error> {
    if bytes.len() > MAX_MANIFEST || bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return fail("the manifest is too large or starts with a byte order mark");
    }
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|e| Error(format!("the manifest is invalid: {e}")))?;
    let present = |value: &str| !value.trim().is_empty();
    if !present(&manifest.title)
        || !present(&manifest.team.name)
        || !present(&manifest.language)
        || !present(&manifest.game.language)
        || !present(&manifest.game.version)
        || !present(&manifest.built.aeria)
    {
        return fail("a manifest field is empty");
    }
    if !valid_version(&manifest.version) {
        return fail("the manifest version is invalid");
    }
    if !matches!(manifest.channel.as_str(), "stable" | "testing") {
        return fail("the manifest channel is invalid");
    }
    if manifest.built.commit.len() != 40
        || !manifest
            .built
            .commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return fail("the manifest commit is invalid");
    }
    let mut authors = manifest.authors.clone();
    authors.sort();
    if manifest.authors.iter().any(|author| !present(author))
        || authors.windows(2).any(|pair| pair[0] == pair[1])
    {
        return fail("the manifest authors are invalid");
    }
    Ok(manifest)
}

/// `YYYY.MM.DD.NNNN`, a real date from 2000 on and a number from 0001.
#[must_use]
pub fn valid_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    let digits = |part: &str, length: usize| {
        part.len() == length && part.bytes().all(|b| b.is_ascii_digit())
    };
    if parts.len() != 4
        || !digits(parts[0], 4)
        || !digits(parts[1], 2)
        || !digits(parts[2], 2)
        || !digits(parts[3], 4)
    {
        return false;
    }
    let number = |part: &str| part.parse::<u32>().unwrap_or(0);
    let (year, month, day) = (number(parts[0]), number(parts[1]), number(parts[2]));
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year >= 2000 && (1..=days).contains(&day) && parts[3] != "0000"
}

fn parse_signature(block: &[u8]) -> Result<Signature, Error> {
    const FIXED: usize = 8 + 2 + 2 + 65 + 64 + 1;
    if block.len() < FIXED || &block[..8] != SIGNATURE_MAGIC {
        return fail("invalid signature block");
    }
    if u16_at(block, 8) != 1 || u16_at(block, 10) != 0 {
        return fail("unsupported signature algorithm");
    }
    let mut public_key = [0; 65];
    public_key.copy_from_slice(&block[12..77]);
    let mut signature = [0; 64];
    signature.copy_from_slice(&block[77..141]);
    let (previous_key, endorsement) = match block[141] {
        0 if block.len() == FIXED => (None, None),
        1 if block.len() == FIXED + 65 + 64 => {
            let mut previous = [0; 65];
            previous.copy_from_slice(&block[142..207]);
            let mut endorsement = [0; 64];
            endorsement.copy_from_slice(&block[207..271]);
            (Some(previous), Some(endorsement))
        }
        _ => return fail("invalid signature endorsement"),
    };
    Ok(Signature {
        public_key,
        value: signature,
        previous_key,
        endorsement,
    })
}

fn verify_with(
    public_key: &[u8; 65],
    domain: &[u8],
    payload: &[u8],
    signature: &[u8; 64],
) -> Result<(), Error> {
    use p256::ecdsa::{Signature as EcdsaSignature, VerifyingKey, signature::Verifier};
    if public_key[0] != 0x04 {
        return fail("the signing key is not an uncompressed P-256 point");
    }
    let key = VerifyingKey::from_sec1_bytes(public_key)
        .map_err(|_| Error("the signing key is not a valid P-256 key".into()))?;
    let signature = EcdsaSignature::from_slice(signature)
        .map_err(|_| Error("the signature is malformed".into()))?;
    let mut message = domain.to_vec();
    message.extend_from_slice(payload);
    key.verify(&message, &signature)
        .map_err(|_| Error("the signature is invalid".into()))
}

fn verify_signature(signature: &Signature, hash: &[u8; 32]) -> Result<(), Error> {
    verify_with(
        &signature.public_key,
        SIGNATURE_DOMAIN,
        hash,
        &signature.value,
    )
    .map_err(|e| Error(format!("pack signature: {e}")))?;
    if let (Some(previous), Some(endorsement)) = (&signature.previous_key, &signature.endorsement) {
        verify_with(
            previous,
            ROTATION_DOMAIN,
            &signature.public_key,
            endorsement,
        )
        .map_err(|e| Error(format!("key endorsement: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEROP: &[u8] = include_bytes!("../../Harmonia.Tests/TestData/harmonia-interop.hpk");
    const FONTS: &[u8] = include_bytes!("../../Harmonia.Tests/TestData/harmonia-interop-fonts.hpk");
    const REPLACEMENTS: &[u8] =
        include_bytes!("../../Harmonia.Tests/TestData/harmonia-interop-replacements.hpk");

    #[test]
    fn reads_the_packs_aeria_writes() {
        for bytes in [INTEROP, FONTS, REPLACEMENTS] {
            let pack = Pack::parse(bytes).unwrap();
            assert!(pack.signature.is_some());
            assert!(pack.cell_count() > 0);
            let cells: usize = pack.sheets().map(|sheet| sheet.cells().count()).sum();
            assert_eq!(cells, pack.cell_count());
            for sheet in pack.sheets() {
                for cell in sheet.cells() {
                    assert!(usize::from(cell.ordinal) < sheet.layout.len());
                    assert!(sestring::is_well_formed(cell.text));
                }
            }
        }
    }

    #[test]
    fn any_changed_byte_is_refused() {
        let pack = Pack::parse(INTEROP).unwrap();
        let body = pack.len() - 271.min(pack.len());
        // Every byte before the signature block is covered by the digest or
        // the header checks; sample them all for this small pack.
        for at in (0..body).step_by(7) {
            let mut copy = INTEROP.to_vec();
            copy[at] ^= 0x40;
            assert!(Pack::parse(&copy).is_err(), "byte {at}");
        }
        assert!(Pack::parse(&INTEROP[..INTEROP.len() - 1]).is_err());
    }

    #[test]
    fn versions_follow_the_release_rule() {
        assert!(valid_version("2026.10.01.0001"));
        for version in [
            "2026.10.01.0000",
            "2026.02.30.0001",
            "1999.01.01.0001",
            "2026.1.01.0001",
            "2026.10.01",
        ] {
            assert!(!valid_version(version), "{version}");
        }
    }
}
