//! A verified pack as an Angelica text table: the host then applies its
//! cells to the rows the game builds, with the same checks Harmonia makes
//! (sheet layout, variant, client language, and the source guard of every
//! cell).
use crate::pack::{GUARD_DOMAIN, Pack};
use angelica_sdk::text::table::{Column, Variant, Writer};

/// The game's language number for a pack's `game.language` tag.
#[must_use]
pub fn game_language(tag: &str) -> Option<u8> {
    let tag = tag.to_ascii_lowercase();
    let primary = tag.split('-').next().unwrap_or("");
    Some(match primary {
        "ja" => 1,
        "en" => 2,
        "de" => 3,
        "fr" => 4,
        "zh" if tag.contains("hant") || tag.ends_with("-tw") || tag.ends_with("-hk") => 6,
        "zh" => 5,
        "ko" => 7,
        _ => return None,
    })
}

/// Whether a translation's language is written in Cyrillic, so the game's
/// letter case needs Cyrillic letters.
#[must_use]
pub fn is_cyrillic(language: &str) -> bool {
    const LANGUAGES: [&str; 13] = [
        "ru", "uk", "be", "bg", "sr", "mk", "kk", "ky", "mn", "tg", "tt", "ba", "cv",
    ];
    let primary = language
        .split('-')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    LANGUAGES.contains(&primary.as_str()) || language.to_ascii_lowercase().contains("cyrl")
}

/// The Cyrillic letters U+0400..U+045F as lower and upper case pairs.
#[must_use]
pub fn cyrillic_letters() -> Vec<(char, char)> {
    ('а'..='я')
        .zip('А'..='Я')
        .chain(('ѐ'..='џ').zip('Ѐ'..='Џ'))
        .collect()
}

/// What the table holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub sheets: usize,
    pub cells: usize,
    /// Sheets the player keeps in the game's language.
    pub kept_sheets: usize,
    /// Cells whose text is a reserved token (`_rsv_…`): the game resolves
    /// and hashes those itself, so they are never written.
    pub reserved: usize,
}

/// Whether the player keeps `sheet` in the game's language: `kept` lists
/// sheet names (`Item`) and folders (`quest/*`).
#[must_use]
pub fn is_kept(sheet: &str, kept: &[String]) -> bool {
    kept.iter().any(|entry| match entry.strip_suffix('*') {
        Some(folder) => sheet.starts_with(folder),
        None => entry == sheet,
    })
}

/// The table for `pack`, without the sheets the player keeps.
///
/// # Errors
/// When the pack's game language is not one the game has, or the pack does
/// not fit the table format.
pub fn compile(pack: &Pack<'_>, kept: &[String]) -> Result<(Vec<u8>, Summary), String> {
    let language = game_language(&pack.manifest.game.language)
        .ok_or_else(|| format!("the game has no language {}", pack.manifest.game.language))?;
    let mut writer = Writer::new(language, GUARD_DOMAIN).map_err(|e| e.to_string())?;
    let mut summary = Summary {
        sheets: 0,
        cells: 0,
        kept_sheets: 0,
        reserved: 0,
    };
    for sheet in pack.sheets() {
        if is_kept(sheet.name, kept) {
            summary.kept_sheets += 1;
            continue;
        }
        let columns: Vec<_> = sheet
            .layout
            .iter()
            .map(|column| Column {
                index: column.index,
                offset: column.offset,
            })
            .collect();
        let variant = if sheet.subrows {
            Variant::Subrows
        } else {
            Variant::Rows
        };
        writer
            .sheet(sheet.name, variant, &columns)
            .map_err(|e| e.to_string())?;
        for cell in sheet.cells() {
            if cell.text.starts_with(b"_rsv_") {
                summary.reserved += 1;
                continue;
            }
            let column = sheet.layout[usize::from(cell.ordinal)].index;
            writer
                .cell(
                    sheet.name,
                    cell.row,
                    cell.subrow,
                    column,
                    cell.guard,
                    cell.text,
                )
                .map_err(|e| e.to_string())?;
            summary.cells += 1;
        }
        summary.sheets += 1;
    }
    Ok((writer.finish().map_err(|e| e.to_string())?, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_sdk::text::table::Table;

    const INTEROP: &[u8] = include_bytes!("../../Harmonia.Tests/TestData/harmonia-interop.hpk");

    #[test]
    fn every_pack_cell_becomes_a_table_cell() {
        let pack = Pack::parse(INTEROP).unwrap();
        let (bytes, summary) = compile(&pack, &[]).unwrap();
        let table = Table::parse(&bytes).unwrap();
        assert_eq!(table.domain(), GUARD_DOMAIN);
        assert_eq!(
            Some(table.language()),
            game_language(&pack.manifest.game.language)
        );
        assert_eq!(summary.cells, pack.cell_count());
        assert_eq!(table.cell_count(), pack.cell_count());
        for sheet in pack.sheets() {
            let listed = table.sheet(sheet.name).unwrap();
            assert_eq!(listed.columns().count(), sheet.layout.len());
            for cell in sheet.cells() {
                let row = listed.row(cell.row, cell.subrow);
                let column = sheet.layout[usize::from(cell.ordinal)].index;
                let found = row.iter().find(|c| c.column == column).unwrap();
                assert_eq!((found.guard, found.text), (cell.guard, cell.text));
            }
        }
    }

    #[test]
    fn kept_sheets_are_left_out() {
        let pack = Pack::parse(INTEROP).unwrap();
        let first = pack.sheets().next().unwrap().name.to_owned();
        let (bytes, summary) = compile(&pack, std::slice::from_ref(&first)).unwrap();
        assert_eq!(summary.kept_sheets, 1);
        assert!(Table::parse(&bytes).unwrap().sheet(&first).is_none());
        assert!(is_kept("quest/001/Text", &["quest/*".into()]));
        assert!(!is_kept("Quest", &["quest/*".into()]));
    }

    #[test]
    fn languages() {
        assert_eq!(game_language("en"), Some(2));
        assert_eq!(game_language("zh-Hant"), Some(6));
        assert_eq!(game_language("ru"), None);
        assert!(is_cyrillic("ru") && is_cyrillic("sr-Cyrl") && !is_cyrillic("en"));
        assert_eq!(cyrillic_letters().len(), 48);
    }
}
