//! A verified pack as an Angelica text table: the host then applies its
//! cells to the rows the game builds, with the same checks Harmonia makes
//! (sheet layout, variant, client language, and the source guard of every
//! cell).
use crate::coverage::SheetKeep;
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

/// What a table leaves out of a pack whatever the coverage; a table of
/// another format is built again.
pub const TABLE_FORMAT: u32 = 1;

/// Text the game sends to the server: the default name an event script reads
/// with `GetAddonText` when the player buys an apartment or private chambers
/// or registers a company airship or submersible. The server refuses a
/// translated name ("Failed to complete housing action."), so these rows stay
/// in the game's language (`HarmoniaEngine`'s `Runtime/ServerNames.cs`).
const SERVER_NAME_SHEET: &str = "Addon";
const SERVER_NAME_ROWS: [u32; 4] = [6487, 6489, 6710, 6888];

#[must_use]
pub fn is_server_name(sheet: &str, row: u32) -> bool {
    sheet == SERVER_NAME_SHEET && SERVER_NAME_ROWS.contains(&row)
}

/// What the table holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub sheets: usize,
    pub cells: usize,
    /// Sheets left whole in the game's language.
    pub kept_sheets: usize,
    /// Cells left in the game's language, whole sheets included.
    #[serde(default)]
    pub kept_cells: usize,
    /// Cells whose text is a reserved token (`_rsv_…`): the game resolves
    /// and hashes those itself, so they are never written.
    pub reserved: usize,
}

/// The table for `pack`, without what `keep` leaves in the game's language
/// and the names the game sends to the server: `keep` is asked once per sheet
/// with the sheet's name and its columns' positions.
///
/// # Errors
/// When the pack's game language is not one the game has, the pack does
/// not fit the table format, or `keep` fails.
pub fn compile(
    pack: &Pack<'_>,
    mut keep: impl FnMut(&str, &[u16]) -> Result<SheetKeep, String>,
) -> Result<(Vec<u8>, Summary), String> {
    let language = game_language(&pack.manifest.game.language)
        .ok_or_else(|| format!("the game has no language {}", pack.manifest.game.language))?;
    let mut writer = Writer::new(language, GUARD_DOMAIN).map_err(|e| e.to_string())?;
    let mut summary = Summary {
        sheets: 0,
        cells: 0,
        kept_sheets: 0,
        kept_cells: 0,
        reserved: 0,
    };
    for sheet in pack.sheets() {
        let positions: Vec<u16> = sheet.layout.iter().map(|column| column.index).collect();
        let kept = match keep(sheet.name, &positions)? {
            SheetKeep::Original => {
                summary.kept_sheets += 1;
                summary.kept_cells += sheet.cells().count();
                continue;
            }
            SheetKeep::Translate => Vec::new(),
            SheetKeep::Columns(columns) => columns,
        };
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
            let column = sheet.layout[usize::from(cell.ordinal)].index;
            if kept.contains(&column) || is_server_name(sheet.name, cell.row) {
                summary.kept_cells += 1;
                continue;
            }
            if cell.text.starts_with(b"_rsv_") {
                summary.reserved += 1;
                continue;
            }
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

    const INTEROP: &[u8] = include_bytes!("../tests/data/harmonia-interop.hpk");

    #[test]
    fn every_pack_cell_becomes_a_table_cell() {
        let pack = Pack::parse(INTEROP).unwrap();
        let (bytes, summary) = compile(&pack, |_, _| Ok(SheetKeep::Translate)).unwrap();
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
    fn kept_sheets_and_columns_are_left_out() {
        let pack = Pack::parse(INTEROP).unwrap();
        let first = pack.sheets().next().unwrap().name.to_owned();
        let (bytes, summary) = compile(&pack, |sheet, _| {
            Ok(if sheet == first {
                SheetKeep::Original
            } else {
                SheetKeep::Translate
            })
        })
        .unwrap();
        assert_eq!(summary.kept_sheets, 1);
        assert!(Table::parse(&bytes).unwrap().sheet(&first).is_none());

        let (bytes, summary) =
            compile(&pack, |_, columns| Ok(SheetKeep::Columns(vec![columns[0]]))).unwrap();
        let table = Table::parse(&bytes).unwrap();
        assert!(summary.kept_cells > 0);
        assert_eq!(summary.cells + summary.kept_cells, pack.cell_count());
        for sheet in pack.sheets() {
            let first = sheet.layout[0].index;
            // A sheet left with no cells is not in the table.
            let Some(listed) = table.sheet(sheet.name) else {
                continue;
            };
            for cell in sheet.cells() {
                let row = listed.row(cell.row, cell.subrow);
                assert!(row.iter().all(|c| c.column != first));
            }
        }
    }

    #[test]
    fn server_names_stay_untranslated() {
        assert!(is_server_name("Addon", 6710) && is_server_name("Addon", 6487));
        assert!(is_server_name("Addon", 6489) && is_server_name("Addon", 6888));
        assert!(!is_server_name("Addon", 6711) && !is_server_name("Item", 6710));
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
