//! What a translation's pack holds, sheet by sheet, for the Coverage tab:
//! rows and cells, each column with the name the game map gives it and
//! whether it holds names or descriptions, a sample cell with its original
//! where the column is named, and the names of the quests whose text
//! sheets the pack has. Built in a job and kept next to the table.
use crate::coverage::{self, Kind};
use serde::{Deserialize, Serialize};

/// The longest sample kept, in characters.
const SAMPLE: usize = 90;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub row: u32,
    /// The game's own text, when the map names the column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub index: u16,
    /// The name the game map gives the column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub kind: Kind,
    pub cells: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<Sample>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    pub rows: u32,
    pub cells: u32,
    pub columns: Vec<Column>,
}

/// A quest whose text sheet (`quest/…/<id>`) the pack has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quest {
    pub id: String,
    pub original: String,
    pub shown: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub hash: String,
    pub game_version: String,
    pub sheets: Vec<Sheet>,
    pub quests: Vec<Quest>,
}

#[must_use]
pub fn path(hash: &str) -> String {
    format!("tables/{hash}.sheets.jsonl")
}

/// The first line of the catalog's file.
#[derive(Serialize, Deserialize)]
struct Header {
    hash: String,
    game_version: String,
}

/// Every other line: a sheet or a quest.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Line {
    Sheet(Sheet),
    Quest(Quest),
}

/// Lines a frame reads of a catalog: a large pack has thousands of
/// sheets, more than one frame of the game can read.
pub const LINES_A_FRAME: usize = 250;

/// A catalog read from its file a few lines a frame.
pub struct Loader {
    text: String,
    at: usize,
    catalog: Catalog,
}

impl Loader {
    /// The catalog in `bytes`, when its first line is a catalog's header.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Option<Self> {
        let text = String::from_utf8(bytes).ok()?;
        let end = text.find('\n').unwrap_or(text.len());
        let header: Header = serde_json::from_str(&text[..end]).ok()?;
        Some(Self {
            catalog: Catalog {
                hash: header.hash,
                game_version: header.game_version,
                ..Catalog::default()
            },
            at: (end + 1).min(text.len()),
            text,
        })
    }

    #[must_use]
    pub fn hash(&self) -> &str {
        &self.catalog.hash
    }

    #[must_use]
    pub fn game_version(&self) -> &str {
        &self.catalog.game_version
    }

    /// How much is read, from 0 to 1.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let fraction = self.at as f32 / self.text.len().max(1) as f32;
        fraction
    }

    /// Reads up to `lines` more lines; the catalog once all are read.
    ///
    /// # Errors
    /// A line that is not a sheet or a quest.
    pub fn step(&mut self, lines: usize) -> Result<Option<Catalog>, String> {
        for _ in 0..lines {
            if self.at >= self.text.len() {
                break;
            }
            let rest = &self.text[self.at..];
            let end = rest.find('\n').unwrap_or(rest.len());
            let line = &rest[..end];
            self.at += end + 1;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(line).map_err(|e| e.to_string())? {
                Line::Sheet(sheet) => self.catalog.sheets.push(sheet),
                Line::Quest(quest) => self.catalog.quests.push(quest),
            }
        }
        Ok((self.at >= self.text.len()).then(|| std::mem::take(&mut self.catalog)))
    }
}

impl Catalog {
    /// The catalog's file: a header line, then a line per sheet and quest.
    ///
    /// # Errors
    /// When a line cannot be written as JSON.
    pub fn to_lines(&self) -> Result<String, String> {
        let mut text = serde_json::to_string(&Header {
            hash: self.hash.clone(),
            game_version: self.game_version.clone(),
        })
        .map_err(|e| e.to_string())?;
        let sheets = self.sheets.iter().cloned().map(Line::Sheet);
        let quests = self.quests.iter().cloned().map(Line::Quest);
        for line in sheets.chain(quests) {
            text.push('\n');
            text.push_str(&serde_json::to_string(&line).map_err(|e| e.to_string())?);
        }
        Ok(text)
    }
}

fn short(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= SAMPLE {
        return text;
    }
    let cut: String = text.chars().take(SAMPLE).collect();
    format!("{}…", cut.trim_end())
}

impl Catalog {
    #[must_use]
    pub fn sheet(&self, name: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|sheet| sheet.name == name)
    }

    /// The sheets an entry (`Item`, `Item:name`, `quest/*`) covers.
    #[must_use]
    pub fn covered(&self, entry: &str) -> Vec<&Sheet> {
        let entry = entry.split_once(':').map_or(entry, |(sheet, _)| sheet);
        match entry.strip_suffix('*') {
            Some(folder) => self
                .sheets
                .iter()
                .filter(|sheet| sheet.name.starts_with(folder))
                .collect(),
            None => self.sheet(entry).into_iter().collect(),
        }
    }

    /// The quest a text sheet is for.
    #[must_use]
    pub fn quest(&self, sheet: &str) -> Option<&Quest> {
        let id = sheet.strip_prefix("quest/")?.rsplit('/').next()?;
        self.quests.iter().find(|quest| quest.id == id)
    }

    /// The kinds of text the entries hold, as far as the pack has them.
    #[must_use]
    pub fn kinds(&self, entries: &[String]) -> (bool, bool) {
        let mut kinds = (false, false);
        for entry in entries {
            if let Some((_, column)) = entry.split_once(':') {
                for sheet in self.covered(entry) {
                    match coverage::kind(&sheet.name, Some(column)) {
                        Kind::Name => kinds.0 = true,
                        Kind::Description => kinds.1 = true,
                    }
                }
                continue;
            }
            for sheet in self.covered(entry) {
                for column in &sheet.columns {
                    match column.kind {
                        Kind::Name => kinds.0 = true,
                        Kind::Description => kinds.1 = true,
                    }
                }
            }
        }
        kinds
    }

    /// Sheets, folders, and quest sheets matching `query` by name, folder,
    /// or quest name: entries to add, best first, at most `limit`, with
    /// how many there are.
    #[must_use]
    pub fn find(&self, query: &str, limit: usize) -> (Vec<String>, usize) {
        let query = crate::names::normalize(query);
        if query.is_empty() {
            return (Vec::new(), 0);
        }
        let mut found: Vec<(u8, String)> = Vec::new();
        let mut folders: Vec<String> = Vec::new();
        for sheet in &self.sheets {
            let name = sheet.name.to_ascii_lowercase();
            let mut parts: Vec<&str> = sheet.name.split('/').collect();
            parts.pop();
            let mut prefix = String::new();
            for part in parts {
                prefix.push_str(part);
                prefix.push('/');
                let folder = format!("{prefix}*");
                if prefix.to_ascii_lowercase().contains(&query) && !folders.contains(&folder) {
                    folders.push(folder);
                }
            }
            let rank = if name == query {
                0
            } else if name.starts_with(&query)
                || name
                    .rsplit('/')
                    .next()
                    .is_some_and(|n| n.starts_with(&query))
            {
                2
            } else if crate::names::normalize(&sheet.name).contains(&query) {
                3
            } else if self.quest(&sheet.name).is_some_and(|quest| {
                crate::names::normalize(&quest.original).contains(&query)
                    || crate::names::normalize(&quest.shown).contains(&query)
            }) {
                4
            } else {
                continue;
            };
            found.push((rank, sheet.name.clone()));
        }
        found.extend(folders.into_iter().map(|folder| (1, folder)));
        found.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.len().cmp(&b.1.len()))
                .then(a.1.cmp(&b.1))
        });
        let total = found.len();
        (
            found
                .into_iter()
                .take(limit)
                .map(|(_, name)| name)
                .collect(),
            total,
        )
    }
}

/// Builds the catalog of the installed pack `hash`: runs in a job.
///
/// # Errors
/// When the pack cannot be read.
pub fn build(hash: &str) -> Result<Catalog, String> {
    use angelica_sdk::excel;
    let bytes = crate::library::read_file(&crate::library::pack_path(hash), crate::pack::MAX_PACK)?;
    let pack = crate::pack::Pack::parse(&bytes).map_err(|e| e.0)?;
    let language = crate::compile::game_language(&pack.manifest.game.language).unwrap_or(2);
    let _ = angelica_sdk::api::jobs::report(0.4, &crate::words("Reading sheets", "Чтение листов"));
    let mut catalog = Catalog {
        hash: hash.to_owned(),
        game_version: angelica_sdk::api::runtime::game_version(),
        ..Catalog::default()
    };
    let mut quest_ids = Vec::new();
    for sheet in pack.sheets() {
        if let Some(id) = sheet
            .name
            .strip_prefix("quest/")
            .and_then(|rest| rest.rsplit('/').next())
        {
            quest_ids.push(id.to_owned());
        }
        // The map names columns of top-level sheets; folders hold dialogue.
        let fields: Vec<(String, u16)> = if sheet.name.contains('/') {
            Vec::new()
        } else {
            excel::sheet(sheet.name)
                .map(|info| info.fields.into_iter().map(|f| (f.name, f.index)).collect())
                .unwrap_or_default()
        };
        let mut columns: Vec<Column> = sheet
            .layout
            .iter()
            .map(|column| {
                let name = fields
                    .iter()
                    .find(|(_, at)| *at == column.index)
                    .map(|(name, _)| name.clone());
                Column {
                    index: column.index,
                    kind: coverage::kind(sheet.name, name.as_deref()),
                    name,
                    cells: 0,
                    sample: None,
                }
            })
            .collect();
        let (mut rows, mut cells, mut last) = (0, 0, None);
        for cell in sheet.cells() {
            cells += 1;
            if last != Some(cell.row) {
                rows += 1;
                last = Some(cell.row);
            }
            let column = &mut columns[usize::from(cell.ordinal)];
            column.cells += 1;
            if column.sample.is_none() && cell.subrow == 0 {
                let text = short(&angelica_sdk::text::sestring::plain(cell.text));
                if text.chars().count() >= 2 {
                    column.sample = Some(Sample {
                        row: cell.row,
                        original: None,
                        text,
                    });
                }
            }
        }
        for column in &mut columns {
            if let (Some(name), Some(sample)) = (&column.name, &mut column.sample)
                && let Ok(Some(values)) = excel::row(sheet.name, language, &[name], sample.row)
            {
                sample.original = values[0].plain().map(|text| short(&text));
            }
        }
        catalog.sheets.push(Sheet {
            name: sheet.name.to_owned(),
            rows,
            cells,
            columns,
        });
    }
    let _ = angelica_sdk::api::jobs::report(0.8, &crate::words("Reading quests", "Чтение квестов"));
    catalog.quests = quests(&pack, language, &quest_ids);
    Ok(catalog)
}

/// The names of the quests `ids`, original and as the translation shows
/// them; none when the map does not name the Quest sheet's `id` and `name`.
fn quests(pack: &crate::pack::Pack<'_>, language: u8, ids: &[String]) -> Vec<Quest> {
    use angelica_sdk::excel;
    let Ok(info) = excel::sheet("Quest") else {
        return Vec::new();
    };
    let Some(name) = info.fields.iter().find(|f| f.name == "name") else {
        return Vec::new();
    };
    if !info.fields.iter().any(|f| f.name == "id") {
        return Vec::new();
    }
    let mut translated = std::collections::HashMap::new();
    if let Some(sheet) = pack.sheets().find(|sheet| sheet.name == "Quest") {
        for cell in sheet.cells() {
            if sheet.layout[usize::from(cell.ordinal)].index == name.index {
                translated.insert(cell.row, (cell.guard, cell.text.to_vec()));
            }
        }
    }
    let wanted: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut quests = Vec::new();
    for row in excel::rows("Quest", language, &["id", "name"]).flatten() {
        let id = row.values[0].plain().unwrap_or_default();
        if !wanted.contains(id.as_str()) {
            continue;
        }
        let Some(source) = row.values[1].text() else {
            continue;
        };
        let original = angelica_sdk::text::sestring::plain(source);
        let shown = translated
            .get(&row.key.row)
            .filter(|(guard, _)| {
                *guard == angelica_sdk::text::guard(crate::pack::GUARD_DOMAIN, source)
            })
            .map_or_else(
                || original.clone(),
                |(_, text)| angelica_sdk::text::sestring::plain(text),
            );
        quests.push(Quest {
            id,
            original,
            shown,
        });
    }
    quests
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(name: &str) -> Sheet {
        Sheet {
            name: name.into(),
            rows: 1,
            cells: 1,
            columns: vec![Column {
                index: 0,
                name: None,
                kind: coverage::kind(name, None),
                cells: 1,
                sample: None,
            }],
        }
    }

    #[test]
    fn a_catalog_is_read_a_few_lines_at_a_time() {
        let catalog = Catalog {
            hash: "h".into(),
            game_version: "v".into(),
            sheets: (0..7).map(|n| sheet(&format!("S{n}"))).collect(),
            quests: vec![Quest {
                id: "a".into(),
                original: "b".into(),
                shown: "c".into(),
            }],
        };
        let mut loader = Loader::new(catalog.to_lines().unwrap().into_bytes()).unwrap();
        assert_eq!(loader.hash(), "h");
        let mut steps = 0;
        let read = loop {
            steps += 1;
            if let Some(read) = loader.step(3).unwrap() {
                break read;
            }
        };
        assert_eq!(steps, 3);
        assert_eq!(read, catalog);
        assert!(Loader::new(b"[]".to_vec()).is_none());
    }

    #[test]
    fn finds_sheets_folders_and_quests() {
        let catalog = Catalog {
            sheets: vec![
                sheet("CraftAction"),
                sheet("CraftType"),
                sheet("quest/000/ClsCrp001_00001"),
                sheet("Item"),
            ],
            quests: vec![Quest {
                id: "ClsCrp001_00001".into(),
                original: "Way of the Carpenter".into(),
                shown: "Путь плотника".into(),
            }],
            ..Catalog::default()
        };
        let (found, total) = catalog.find("craft", 10);
        assert_eq!(total, 2);
        assert_eq!(found, ["CraftType", "CraftAction"]);
        assert_eq!(
            catalog.find("quest", 10).0,
            ["quest/*", "quest/000/*", "quest/000/ClsCrp001_00001"]
        );
        assert_eq!(catalog.find("плотник", 10).0, ["quest/000/ClsCrp001_00001"]);
        assert_eq!(catalog.covered("quest/*").len(), 1);
        assert_eq!(catalog.kinds(&["Item".into()]), (false, true));
        assert_eq!(catalog.kinds(&["Item:name".into()]), (true, false));
    }
}
