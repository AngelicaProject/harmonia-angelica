//! The name dictionary: every name of items, actions, statuses, places,
//! duties, and quests in the game's language, in the translation, and in
//! the languages the player chose, searchable by any of them (the rules of
//! `HarmoniaEngine`'s `Dictionary/NameIndex.cs` and `NameText.cs`).
//!
//! The index is built in a job from the game's sheets and the applied
//! translation and kept in storage as lines; a search is a job too, so the
//! game's frame never holds a hundred thousand names.
use crate::coverage::{self, Coverage, SheetKeep};
use serde::{Deserialize, Serialize};

pub const INDEX: &str = "names/index.txt";

/// What the index holds and how; a new one rebuilds an index made before.
const FORMAT: u32 = 3;
/// The fewest letters a search needs.
pub const MIN_QUERY: usize = 2;

/// In result order when matches are equally good.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Category {
    Item,
    Action,
    Status,
    Place,
    Duty,
    Quest,
}

impl Category {
    pub const ALL: [Self; 6] = [
        Self::Item,
        Self::Action,
        Self::Status,
        Self::Place,
        Self::Duty,
        Self::Quest,
    ];

    /// The sheet the names come from.
    #[must_use]
    pub fn sheet(self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::Action => "Action",
            Self::Status => "Status",
            Self::Place => "PlaceName",
            Self::Duty => "ContentFinderCondition",
            Self::Quest => "Quest",
        }
    }

    /// Where the category's rows take their icons from.
    fn icons(self) -> Icons {
        match self {
            Self::Item | Self::Action | Self::Status => Icons::Own,
            Self::Quest => Icons::Through("genre", "JournalGenre"),
            Self::Duty => Icons::Through("content_type", "ContentType"),
            Self::Place => Icons::All(PLACE_ICON),
        }
    }

    fn number(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    fn from_number(number: usize) -> Option<Self> {
        Self::ALL.get(number).copied()
    }
}

/// The game's map flag, for places, whose rows have no icon of their own.
const PLACE_ICON: u32 = 60561;

/// Where a category's rows take their icons from.
enum Icons {
    /// Their own `icon` column.
    Own,
    /// A column naming a row of another sheet, whose `icon` it is.
    Through(&'static str, &'static str),
    /// One icon for every row.
    All(u32),
}

/// The icons of a sheet's rows by row, from its `icon` column.
fn icons_of(
    sheet: &str,
    language: u8,
    notes: &mut Vec<String>,
) -> std::collections::HashMap<u32, u32> {
    let mut icons = std::collections::HashMap::new();
    for row in angelica_sdk::excel::rows(sheet, language, &["icon"]) {
        match row {
            Ok(row) => {
                if let Some(icon) = row.values[0]
                    .unsigned()
                    .and_then(|icon| u32::try_from(icon).ok())
                {
                    icons.insert(row.key.row, icon);
                }
            }
            Err(error) => {
                notes.push(format!("{sheet}: {}", error.message));
                break;
            }
        }
    }
    icons
}

/// A name in one of the game's languages (`ja`, `en`, `de`, `fr`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Original {
    pub language: String,
    pub text: String,
}

/// A name: what the game shows with the translation, and the originals,
/// the game's language first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Name {
    pub category: Category,
    pub row: u32,
    /// The game's icon number; 0 for none.
    pub icon: u32,
    pub shown: String,
    pub originals: Vec<Original>,
    /// The name in English, for pages about it; empty when the game has
    /// none.
    #[serde(default)]
    pub english: String,
    /// An item that can be worn or held, and so tried on.
    #[serde(default)]
    pub wearable: bool,
}

impl Name {
    /// The name in the game's language.
    #[must_use]
    pub fn original(&self) -> &str {
        self.originals.first().map_or(&self.shown, |o| &o.text)
    }

    #[must_use]
    pub fn is_translated(&self) -> bool {
        self.shown != self.original()
    }
}

/// Tags and numbers of the game's languages a dictionary can show.
pub const LANGUAGES: [(&str, u8); 4] = [("ja", 1), ("en", 2), ("de", 3), ("fr", 4)];

/// The game's number for English.
const ENGLISH: u8 = 2;

/// The language tag for a game language number.
#[must_use]
pub fn tag(language: u8) -> &'static str {
    LANGUAGES
        .iter()
        .find(|(_, number)| *number == language)
        .map_or("en", |(tag, _)| tag)
}

// Standard Russian and US layouts, key by key; the shifted punctuation keys
// carry the letters Х, Ъ, Ж, Э, Б, Ю, and Ё.
const LATIN: &str = "`qwertyuiop[]asdfghjkl;'zxcvbnm,.~{}:\"<>";
const CYRILLIC: &str = "ёйцукенгшщзхъфывапролджэячсмитьбюёхъжэбю";

/// A Latin letter with a mark, as its base letter.
fn base_letter(ch: char) -> Option<char> {
    const FOLDS: [(&str, char); 19] = [
        ("àáâãäåāăą", 'a'),
        ("çćĉċč", 'c'),
        ("ďđ", 'd'),
        ("èéêëēĕėęě", 'e'),
        ("ĝğġģ", 'g'),
        ("ĥħ", 'h'),
        ("ìíîïĩīĭįı", 'i'),
        ("ĵ", 'j'),
        ("ķ", 'k'),
        ("ĺļľŀł", 'l'),
        ("ñńņňŉ", 'n'),
        ("òóôõöøōŏő", 'o'),
        ("ŕŗř", 'r'),
        ("śŝşš", 's'),
        ("ţťŧ", 't'),
        ("ùúûüũūŭůűų", 'u'),
        ("ŵ", 'w'),
        ("ýÿŷ", 'y'),
        ("źżž", 'z'),
    ];
    FOLDS
        .iter()
        .find(|(marked, _)| marked.contains(ch))
        .map(|(_, base)| *base)
}

/// The search key of a text: lowercase letters and digits separated by
/// single spaces. Marks of Latin letters, apostrophes, and the game's icon
/// glyphs (the HQ and collectable marks in the Private Use Area) are
/// dropped; full-width Latin is plain Latin; ё is е.
#[must_use]
pub fn normalize(text: &str) -> String {
    let mut key = String::with_capacity(text.len());
    let mut separated = false;
    for ch in text.chars() {
        let ch = match ch {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
            _ => ch,
        };
        if matches!(ch, '\'' | '’' | '‘' | 'ʼ' | '`' | '\u{E000}'..='\u{F8FF}') {
            continue;
        }
        if !ch.is_alphanumeric() {
            separated = true;
            continue;
        }
        if separated && !key.is_empty() {
            key.push(' ');
        }
        separated = false;
        for lower in ch.to_lowercase() {
            key.push(match lower {
                'ё' => 'е',
                other => base_letter(other).unwrap_or(other),
            });
        }
    }
    key
}

/// The text as if typed with the other keyboard layout ("Скувутвгь" for
/// "Credendum" and back), or `None` when no key maps. The direction follows
/// the script most of the letters are in.
#[must_use]
pub fn swap_layout(text: &str) -> Option<String> {
    let latin = text.chars().filter(char::is_ascii_alphabetic).count();
    let cyrillic = text
        .chars()
        .filter(|c| ('\u{400}'..='\u{4FF}').contains(c))
        .count();
    if latin == cyrillic {
        return None;
    }
    let (from, to): (Vec<char>, Vec<char>) = if latin > cyrillic {
        (LATIN.chars().collect(), CYRILLIC.chars().collect())
    } else {
        (CYRILLIC.chars().collect(), LATIN.chars().collect())
    };
    let mut changed = false;
    let swapped = text
        .chars()
        .map(|ch| {
            let lower = ch.to_lowercase().next().unwrap_or(ch);
            from.iter().position(|&c| c == lower).map_or(ch, |at| {
                changed = true;
                to[at]
            })
        })
        .collect();
    changed.then_some(swapped)
}

const EXACT: i32 = 1000;
const PREFIX: i32 = 800;
const WORDS: i32 = 600;
const SUBSTRING: i32 = 500;
const FUZZY: i32 = 300;
const SWAPPED_PENALTY: i32 = 50;
const MAX_FUZZY: usize = 48;

/// One line of the index: a name and its search keys.
struct Item {
    name: Name,
    forms: Vec<String>,
}

const FIELD: char = '\t';
const PART: char = '\u{1E}';
const PAIR: char = '\u{1F}';

fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

impl Item {
    fn new(name: Name) -> Option<Self> {
        let mut forms = vec![normalize(&name.shown)];
        for original in &name.originals {
            let form = normalize(&original.text);
            if !forms.contains(&form) {
                forms.push(form);
            }
        }
        forms.retain(|form| !form.is_empty());
        (!forms.is_empty()).then_some(Self { name, forms })
    }

    fn line(&self) -> String {
        let originals: Vec<String> = self
            .name
            .originals
            .iter()
            .map(|o| format!("{}{PAIR}{}", o.language, clean(&o.text)))
            .collect();
        format!(
            "{}{FIELD}{}{FIELD}{}{FIELD}{}{FIELD}{}{FIELD}{}{FIELD}{}{FIELD}{}",
            self.name.category.number(),
            self.name.row,
            self.name.icon,
            clean(&self.name.shown),
            originals.join(&PART.to_string()),
            self.forms.join(&PART.to_string()),
            clean(&self.name.english),
            u8::from(self.name.wearable),
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let mut fields = line.split(FIELD);
        let category = Category::from_number(fields.next()?.parse().ok()?)?;
        let row = fields.next()?.parse().ok()?;
        let icon = fields.next()?.parse().ok()?;
        let shown = fields.next()?.to_owned();
        let originals = fields
            .next()?
            .split(PART)
            .filter_map(|pair| {
                let (language, text) = pair.split_once(PAIR)?;
                Some(Original {
                    language: language.to_owned(),
                    text: text.to_owned(),
                })
            })
            .collect();
        let forms = fields.next()?.split(PART).map(str::to_owned).collect();
        let english = fields.next().unwrap_or_default().to_owned();
        let wearable = fields.next() == Some("1");
        Some(Self {
            name: Name {
                category,
                row,
                icon,
                shown,
                originals,
                english,
                wearable,
            },
            forms,
        })
    }
}

/// What a search found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub query: String,
    pub matches: Vec<Name>,
    /// The matches of the searched category.
    pub total: usize,
    /// The matches of every category, whatever was searched.
    pub by_category: [usize; 6],
}

struct Query {
    text: String,
    words: Vec<String>,
    penalty: i32,
}

/// Every query word starts a different word of the name, in any order.
fn all_words_start(text: &str, words: &[String]) -> bool {
    let parts: Vec<&str> = text.split(' ').collect();
    if words.len() > parts.len() {
        return false;
    }
    let mut used = vec![false; parts.len()];
    words.iter().all(|word| {
        parts.iter().enumerate().any(|(i, part)| {
            let free = !used[i] && part.starts_with(word.as_str());
            if free {
                used[i] = true;
            }
            free
        })
    })
}

/// As typed: 0 when only a typo pass could match.
fn score(text: &str, query: &Query) -> i32 {
    if text == query.text {
        return EXACT;
    }
    if !query.words.iter().all(|word| text.contains(word.as_str())) {
        return 0;
    }
    if text.starts_with(&query.text) {
        PREFIX
    } else if all_words_start(text, &query.words) {
        WORDS
    } else if text.contains(&query.text) {
        SUBSTRING
    } else {
        0
    }
}

/// Edit distance with adjacent transpositions; above `limit` is `limit + 1`.
fn distance(a: &[char], b: &[char], limit: usize) -> usize {
    if a.len() > MAX_FUZZY || b.len() > MAX_FUZZY {
        return limit + 1;
    }
    let width = b.len() + 1;
    let mut before = vec![0; width];
    let mut previous: Vec<usize> = (0..width).collect();
    let mut current = vec![0; width];
    for i in 1..=a.len() {
        current[0] = i;
        let mut least = i;
        for j in 1..width {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut value = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                value = value.min(before[j - 2] + 1);
            }
            current[j] = value;
            least = least.min(value);
        }
        if least > limit {
            return limit + 1;
        }
        std::mem::swap(&mut before, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()].min(limit + 1)
}

/// Every query word is a word of the name with a typo or two; short words
/// must start one. Fewer typos score higher.
fn fuzzy(text: &str, words: &[String]) -> i32 {
    let mut total = 0;
    for query in words {
        let q: Vec<char> = query.chars().collect();
        let allowed = match q.len() {
            8.. => 2,
            4..=7 => 1,
            _ => 0,
        };
        let mut best = usize::MAX;
        for word in text.split(' ') {
            let w: Vec<char> = word.chars().collect();
            if allowed == 0 {
                if w.starts_with(&q) {
                    best = 0;
                }
            } else if w.len().abs_diff(q.len()) <= allowed {
                best = best.min(distance(&w, &q, allowed));
            } else if w.len() > q.len() && distance(&w[..q.len()], &q, allowed) <= allowed {
                // A typo in a word that is still being typed.
                best = best.min(allowed);
            }
            if best == 0 {
                break;
            }
        }
        if best > allowed {
            return 0;
        }
        total += i32::try_from(best).unwrap_or(i32::MAX / 20);
    }
    FUZZY - 10 * total
}

/// Searches the index's lines for `query`, `limit` best first; `category`
/// `None` searches every category.
#[must_use]
pub fn search(index: &str, query: &str, category: Option<Category>, limit: usize) -> Found {
    let mut found = Found {
        query: query.to_owned(),
        ..Found::default()
    };
    let mut queries = Vec::new();
    let normalized = normalize(query);
    if normalized.chars().count() >= MIN_QUERY {
        queries.push(Query {
            words: normalized.split(' ').map(str::to_owned).collect(),
            text: normalized.clone(),
            penalty: 0,
        });
    }
    if let Some(other) = swap_layout(query).map(|s| normalize(&s))
        && other.chars().count() >= MIN_QUERY
        && other != normalized
    {
        queries.push(Query {
            words: other.split(' ').map(str::to_owned).collect(),
            text: other,
            penalty: SWAPPED_PENALTY,
        });
    }
    if queries.is_empty() {
        return found;
    }
    let items: Vec<Item> = index.lines().filter_map(Item::parse).collect();
    // Typos count only when nothing matches as typed.
    let mut scored = collect(&items, &queries, false);
    if scored.is_empty() {
        scored = collect(&items, &queries, true);
    }
    for (item, _, _) in &scored {
        found.by_category[item.name.category.number()] += 1;
    }
    if let Some(only) = category {
        scored.retain(|(item, _, _)| item.name.category == only);
    }
    found.total = scored.len();
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.2.cmp(&b.2))
            .then(a.0.name.category.cmp(&b.0.name.category))
            .then(a.0.name.row.cmp(&b.0.name.row))
    });
    found.matches = scored
        .into_iter()
        .take(limit)
        .map(|(item, _, _)| item.name.clone())
        .collect();
    found
}

fn collect<'a>(items: &'a [Item], queries: &[Query], typos: bool) -> Vec<(&'a Item, i32, usize)> {
    let mut scored = Vec::new();
    for item in items {
        let mut best = 0;
        let mut closeness = usize::MAX;
        for query in queries {
            for form in &item.forms {
                let mut points = if typos {
                    fuzzy(form, &query.words)
                } else {
                    score(form, query)
                };
                if points == 0 {
                    continue;
                }
                points -= query.penalty;
                let near = form.chars().count().abs_diff(query.text.chars().count());
                if points > best || (points == best && near < closeness) {
                    best = points;
                    closeness = near;
                }
            }
        }
        if best > 0 {
            scored.push((item, best, closeness));
        }
    }
    scored
}

/// What the index is built from: when any of it changes, it is built again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    /// The applied translation's pack hash.
    pub hash: Option<String>,
    pub coverage: Coverage,
    /// The game's language number.
    pub language: u8,
    /// Other languages shown, by number.
    pub others: Vec<u8>,
    pub game_version: String,
}

impl Build {
    /// The first line of the index file.
    #[must_use]
    pub fn header(&self) -> String {
        format!(
            "{FORMAT}|{}|{}|{}|{:?}|{}",
            self.hash.as_deref().unwrap_or("-"),
            self.coverage.fingerprint(),
            self.language,
            self.others,
            self.game_version
        )
    }
}

/// What the build left out, for the log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Built {
    pub names: usize,
    /// Categories whose sheet the game map does not name a `name` column of.
    pub missing: Vec<Category>,
    pub notes: Vec<String>,
}

/// One sheet's names: the translated text of its `name` column, by row.
pub type Translated = std::collections::HashMap<u32, String>;

/// The index's lines for the given names, deduplicated: names that read
/// the same in every language collapse into the first.
#[must_use]
pub fn lines(names: Vec<Name>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    names
        .into_iter()
        .filter_map(Item::new)
        .filter(|item| seen.insert((item.name.category, item.forms.join("\n"))))
        .map(|item| item.line())
        .collect()
}

/// Whether the translation's text of a sheet's `name` column shows, given
/// what the player keeps in the game's language.
#[must_use]
pub fn translated_names(keep: &SheetKeep, name_column: u16) -> bool {
    match keep {
        SheetKeep::Translate => true,
        SheetKeep::Original => false,
        SheetKeep::Columns(kept) => !kept.contains(&name_column),
    }
}

/// Builds the index's lines from the game's sheets and the applied
/// translation: runs in a job.
///
/// # Errors
/// When the translation's pack cannot be read or a sheet read fails midway.
#[allow(clippy::too_many_lines)]
pub fn build(input: &Build) -> Result<(Vec<String>, Built), String> {
    use angelica_sdk::excel;
    let bytes = match &input.hash {
        Some(hash) => Some(crate::library::read_file(
            &crate::library::pack_path(hash),
            crate::pack::MAX_PACK,
        )?),
        None => None,
    };
    let pack = match &bytes {
        Some(bytes) => Some(crate::pack::Pack::parse(bytes).map_err(|e| e.0)?),
        None => None,
    };
    let groups = input.coverage.groups();
    let mut built = Built::default();
    let mut names = Vec::new();
    for (step, category) in Category::ALL.into_iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let _ = angelica_sdk::api::jobs::report(
            0.3 + 0.6 * step as f32 / 6.0,
            &crate::words("Reading names", "Чтение названий"),
        );
        let sheet = category.sheet();
        let info = match excel::sheet(sheet) {
            Ok(info) => info,
            Err(error) => {
                built.notes.push(format!("{sheet}: {}", error.message));
                built.missing.push(category);
                continue;
            }
        };
        let Some(name_field) = info.fields.iter().find(|f| f.name == "name") else {
            built.missing.push(category);
            continue;
        };
        let has = |name: &str| info.fields.iter().any(|f| f.name == name);
        let (link, through) = match category.icons() {
            Icons::Own if has("icon") => (Some("icon"), None),
            Icons::Through(column, other) if has(column) => (
                Some(column),
                Some(icons_of(other, input.language, &mut built.notes)),
            ),
            _ => (None, None),
        };
        let wear = category == Category::Item && has("equip_slot");
        let columns: Vec<&str> = std::iter::once("name")
            .chain(link)
            .chain(wear.then_some("equip_slot"))
            .collect();
        let mut translated: std::collections::HashMap<u32, (u64, &[u8])> =
            std::collections::HashMap::new();
        if let Some(pack) = &pack
            && let Some(own) = pack.sheets().find(|s| s.name == sheet)
        {
            let positions: Vec<u16> = own.layout.iter().map(|c| c.index).collect();
            let given = || {
                Ok(info
                    .fields
                    .iter()
                    .map(|f| (f.name.clone(), f.index))
                    .collect())
            };
            let keep = coverage::sheet_keep(&groups, sheet, &positions, given)?;
            if translated_names(&keep, name_field.index) {
                for cell in own.cells() {
                    if cell.subrow == 0
                        && own.layout[usize::from(cell.ordinal)].index == name_field.index
                    {
                        translated.insert(cell.row, (cell.guard, cell.text));
                    }
                }
            }
        }
        // English names are read for pages about a name even when the
        // player does not show them.
        let english_hidden = input.language != ENGLISH && !input.others.contains(&ENGLISH);
        let mut others: Vec<(&str, std::collections::HashMap<u32, String>)> = Vec::new();
        let mut english: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
        for language in input
            .others
            .iter()
            .copied()
            .chain(english_hidden.then_some(ENGLISH))
        {
            let mut texts = std::collections::HashMap::new();
            for row in excel::rows(sheet, language, &["name"]) {
                match row {
                    Ok(row) => {
                        if let Some(text) = row.values[0].plain().filter(|t| !t.is_empty()) {
                            texts.insert(row.key.row, text);
                        }
                    }
                    Err(error) => {
                        built.notes.push(format!(
                            "{sheet} in {}: {}",
                            tag(language),
                            error.message
                        ));
                        break;
                    }
                }
            }
            if english_hidden && language == ENGLISH {
                english = texts;
            } else {
                others.push((tag(language), texts));
            }
        }
        for row in excel::rows(sheet, input.language, &columns) {
            let row = row.map_err(|e| format!("{sheet}: {}", e.message))?;
            if row.key.subrow != 0 {
                continue;
            }
            let Some(source) = row.values[0].text() else {
                continue;
            };
            let original = angelica_sdk::text::sestring::plain(source);
            if original.trim().is_empty() {
                continue;
            }
            let shown = translated
                .get(&row.key.row)
                .filter(|(guard, _)| {
                    *guard == angelica_sdk::text::guard(crate::pack::GUARD_DOMAIN, source)
                })
                .map(|(_, text)| angelica_sdk::text::sestring::plain(text))
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| original.clone());
            let mut originals = vec![Original {
                language: tag(input.language).to_owned(),
                text: original.trim().to_owned(),
            }];
            for (language, texts) in &others {
                if let Some(text) = texts.get(&row.key.row) {
                    originals.push(Original {
                        language: (*language).to_owned(),
                        text: text.trim().to_owned(),
                    });
                }
            }
            let english = if input.language == ENGLISH {
                original.trim().to_owned()
            } else {
                originals
                    .iter()
                    .find(|o| o.language == tag(ENGLISH))
                    .map(|o| o.text.clone())
                    .or_else(|| english.get(&row.key.row).map(|t| t.trim().to_owned()))
                    .unwrap_or_default()
            };
            let wearable = wear
                && row
                    .values
                    .last()
                    .and_then(excel::Value::unsigned)
                    .is_some_and(|slot| slot != 0);
            names.push(Name {
                category,
                row: row.key.row,
                icon: match category.icons() {
                    Icons::All(icon) => icon,
                    _ if link.is_none() => 0,
                    _ => row
                        .values
                        .get(1)
                        .and_then(excel::Value::unsigned)
                        .and_then(|value| u32::try_from(value).ok())
                        .map_or(0, |value| match &through {
                            Some(icons) => icons.get(&value).copied().unwrap_or(0),
                            None => value,
                        }),
                },
                shown: shown.trim().to_owned(),
                originals,
                english,
                wearable,
            });
        }
    }
    let lines = lines(names);
    built.names = lines.len();
    Ok((lines, built))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(category: Category, row: u32, shown: &str, original: &str) -> Name {
        Name {
            category,
            row,
            icon: 0,
            shown: shown.into(),
            originals: vec![Original {
                language: "en".into(),
                text: original.into(),
            }],
            english: original.into(),
            wearable: false,
        }
    }

    fn index() -> String {
        lines(vec![
            name(Category::Item, 1, "Мифриловый слиток", "Mythrite Ingot"),
            name(Category::Item, 2, "Мифриловая руда", "Mythrite Ore"),
            name(Category::Place, 3, "Мифриловый рудник", "Mythrite Mine"),
            name(Category::Action, 4, "Кредендум", "Credendum"),
            name(Category::Action, 5, "Кредендум", "Credendum"),
        ])
        .join("\n")
    }

    #[test]
    fn keys() {
        assert_eq!(normalize("Mythrite  Ingot"), "mythrite ingot");
        assert_eq!(normalize("Ёлка's Café"), "елкаs cafe");
        assert_eq!(normalize("Ｍｉｔｈｒｉｌ\u{E03C}"), "mithril");
        assert_eq!(swap_layout("Скувутвгь").as_deref(), Some("credendum"));
        assert_eq!(swap_layout("123"), None);
    }

    #[test]
    fn finds_by_any_language_layout_and_typo() {
        let index = index();
        let found = search(&index, "мифрил", None, 10);
        assert_eq!(found.total, 3);
        assert_eq!(found.by_category[Category::Item.number()], 2);
        let found = search(&index, "mythrite ingot", None, 10);
        assert_eq!(found.matches[0].row, 1);
        // The same name of two rows is listed once.
        assert_eq!(search(&index, "credendum", None, 10).total, 1);
        assert_eq!(search(&index, "Скувутвгь", None, 10).total, 1);
        assert_eq!(search(&index, "mythrtie", None, 10).total, 3);
        let places = search(&index, "мифрил", Some(Category::Place), 10);
        assert_eq!((places.total, places.by_category[0]), (1, 2));
        assert!(search(&index, "м", None, 10).matches.is_empty());
    }
}
