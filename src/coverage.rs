//! What a translation leaves in the game's language: groups of sheets,
//! each with one choice for its names and one for its descriptions, and
//! any column of a sheet set apart from its group.
//!
//! A group holds entries: a sheet (`Item`), one column of a sheet
//! (`Item:description`), or a folder of sheets (`quest/*`). Harmonia ships
//! ten groups, the same sets as `HarmoniaEngine`'s `Packs/SheetGroups.cs`;
//! the player renames them, changes their sheets, removes them, restores
//! them, and makes groups of their own. The settings keep only what the
//! player changed, so a built-in group Harmonia changes later reaches the
//! player unless they changed it themselves.
//!
//! Whether a column is a name or a description comes from the column
//! names the game map gives (`excel.sheet`): see [`kind`].
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What happens to a kind of text of a group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Keep {
    /// The translation shows.
    #[default]
    Translate,
    /// The game's own text shows.
    Original,
}

/// Names (what something is called) or descriptions (any other text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Name,
    Description,
}

/// Columns, by the names the game map gives, that hold names.
const NAME_COLUMNS: [&str; 7] = [
    "name",
    "singular",
    "plural",
    "short_name",
    "name_short",
    "masculine",
    "feminine",
];

/// Columns, by the names the game map gives, that hold descriptions even
/// in a sheet of names.
const DESCRIPTION_COLUMNS: [&str; 3] = ["description", "help", "text"];

/// Sheets whose text is names: what a column the map does not name holds
/// there.
const NAME_SHEETS: [&str; 17] = [
    "Action",
    "Aetheryte",
    "BNpcName",
    "ClassJob",
    "ClassJobCategory",
    "Companion",
    "ContentFinderCondition",
    "ENpcResident",
    "EventItem",
    "GlassesStyle",
    "Mount",
    "Ornament",
    "PlaceName",
    "Quest",
    "Title",
    "Town",
    "Trait",
];

/// Whether a text column of `sheet`, named `column` by the game map (or
/// not named), holds names or descriptions. A named column is looked up
/// by its name; an unnamed one is a name in the sheets of names and a
/// description elsewhere.
#[must_use]
pub fn kind(sheet: &str, column: Option<&str>) -> Kind {
    match column {
        Some(name) if NAME_COLUMNS.contains(&name) => Kind::Name,
        Some(name) if DESCRIPTION_COLUMNS.contains(&name) => Kind::Description,
        _ if NAME_SHEETS.contains(&sheet) => Kind::Name,
        _ => Kind::Description,
    }
}

/// A group Harmonia ships.
pub struct BuiltIn {
    pub key: &'static str,
    pub en: &'static str,
    pub ru: &'static str,
    pub entries: &'static [&'static str],
}

/// Names first, then descriptions and story text.
pub const BUILT_IN: [BuiltIn; 10] = [
    BuiltIn {
        key: "items",
        en: "Items",
        ru: "Предметы",
        entries: &["Item", "EventItem", "EventItemHelp"],
    },
    BuiltIn {
        key: "duties",
        en: "Duties",
        ru: "Подземелья, испытания и рейды",
        entries: &[
            "ContentFinderCondition",
            "ContentFinderConditionTransient",
            "ContentRoulette",
            "InstanceContent",
        ],
    },
    BuiltIn {
        key: "places",
        en: "Places",
        ru: "Места",
        entries: &["PlaceName", "Town", "Aetheryte"],
    },
    BuiltIn {
        key: "actions",
        en: "Actions and statuses",
        ru: "Умения и эффекты",
        entries: &[
            "Action",
            "ActionTransient",
            "Trait",
            "TraitTransient",
            "Status",
            "CraftAction",
            "GeneralAction",
            "PetAction",
            "BuddyAction",
            "CompanyAction",
        ],
    },
    BuiltIn {
        key: "jobs",
        en: "Classes and jobs",
        ru: "Классы и профессии",
        entries: &["ClassJob", "ClassJobCategory"],
    },
    BuiltIn {
        key: "characters",
        en: "Character names",
        ru: "Имена персонажей",
        entries: &["ENpcResident", "BNpcName"],
    },
    BuiltIn {
        key: "collections",
        en: "Mounts, minions, and accessories",
        ru: "Ездовые, компаньоны и аксессуары",
        entries: &[
            "Mount",
            "MountTransient",
            "Companion",
            "CompanionTransient",
            "Ornament",
            "OrnamentTransient",
            "Glasses",
            "GlassesStyle",
        ],
    },
    BuiltIn {
        key: "achievements",
        en: "Achievements and titles",
        ru: "Достижения и титулы",
        entries: &["Achievement", "Title"],
    },
    BuiltIn {
        key: "quest_names",
        en: "Quest names",
        ru: "Названия квестов",
        entries: &["Quest"],
    },
    BuiltIn {
        key: "story",
        en: "Story and dialogue",
        ru: "Сюжет и диалоги",
        entries: &["quest/*", "cut_scene/*", "custom/*"],
    },
];

fn built_in(key: &str) -> Option<&'static BuiltIn> {
    BUILT_IN.iter().find(|group| group.key == key)
}

/// What the player changed of a built-in group, or a group of their own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedGroup {
    pub key: String,
    /// The player's name for the group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The group's entries, when the player changed a built-in group's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<String>>,
    #[serde(default)]
    pub names: Keep,
    #[serde(default)]
    pub descriptions: Keep,
    /// Columns (`Item:description`) set apart from the group's choice.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub columns: BTreeMap<String, Keep>,
    /// A built-in group the player removed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
}

/// The player's groups, as saved in the settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    #[serde(default)]
    pub groups: Vec<SavedGroup>,
}

/// A group as the player sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub key: String,
    /// The player's name; a built-in group without one has its own.
    pub name: Option<String>,
    pub built_in: bool,
    pub entries: Vec<String>,
    pub names: Keep,
    pub descriptions: Keep,
    pub columns: BTreeMap<String, Keep>,
    /// A built-in group whose name, entries, or columns the player changed.
    pub changed: bool,
}

/// What an entry covers of a sheet.
enum Covers<'a> {
    No,
    Sheet,
    Column(&'a str),
}

fn covers<'a>(entry: &'a str, sheet: &str) -> Covers<'a> {
    if let Some(folder) = entry.strip_suffix('*') {
        if sheet.starts_with(folder) {
            return Covers::Sheet;
        }
    } else if let Some((name, column)) = entry.split_once(':') {
        if name == sheet {
            return Covers::Column(column);
        }
    } else if entry == sheet {
        return Covers::Sheet;
    }
    Covers::No
}

impl Group {
    /// The group's name in the interface's language.
    #[must_use]
    pub fn title(&self, russian: bool) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        built_in(&self.key).map_or_else(
            || self.key.clone(),
            |group| if russian { group.ru } else { group.en }.to_owned(),
        )
    }

    /// The choice for a kind of text.
    #[must_use]
    pub fn choice(&self, kind: Kind) -> Keep {
        match kind {
            Kind::Name => self.names,
            Kind::Description => self.descriptions,
        }
    }

    /// Whether the group touches `sheet` at all.
    #[must_use]
    pub fn has(&self, sheet: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| !matches!(covers(entry, sheet), Covers::No))
    }

    /// What the group decides for a column of `sheet` that the map names
    /// `column` and that holds `kind`; `None` when it does not cover it.
    #[must_use]
    pub fn decides(&self, sheet: &str, column: Option<&str>, kind: Kind) -> Option<Keep> {
        let mut decision = None;
        for entry in &self.entries {
            let applies = match covers(entry, sheet) {
                Covers::No => false,
                Covers::Sheet => true,
                Covers::Column(named) => column == Some(named),
            };
            if applies {
                let own = column
                    .and_then(|column| self.columns.get(&format!("{sheet}:{column}")))
                    .copied()
                    .unwrap_or(self.choice(kind));
                if own == Keep::Original {
                    return Some(own);
                }
                decision = Some(own);
            }
        }
        decision
    }
}

/// What the table keeps of a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SheetKeep {
    /// Every cell is translated.
    Translate,
    /// The whole sheet stays in the game's language.
    Original,
    /// These columns, by their position, stay in the game's language.
    Columns(Vec<u16>),
}

impl Coverage {
    /// The groups as the player sees them: the built-in ones with the
    /// player's changes, then the player's own in the order they were made.
    #[must_use]
    pub fn groups(&self) -> Vec<Group> {
        let mut groups = Vec::new();
        for shipped in &BUILT_IN {
            let saved = self.groups.iter().find(|g| g.key == shipped.key);
            if saved.is_some_and(|g| g.removed) {
                continue;
            }
            let default = SavedGroup::default();
            let saved = saved.unwrap_or(&default);
            groups.push(Group {
                key: shipped.key.to_owned(),
                name: saved.name.clone(),
                built_in: true,
                entries: saved
                    .entries
                    .clone()
                    .unwrap_or_else(|| shipped.entries.iter().map(|&e| e.to_owned()).collect()),
                names: saved.names,
                descriptions: saved.descriptions,
                columns: saved.columns.clone(),
                changed: saved.name.is_some()
                    || saved.entries.is_some()
                    || !saved.columns.is_empty(),
            });
        }
        for own in &self.groups {
            if built_in(&own.key).is_some() || own.removed {
                continue;
            }
            groups.push(Group {
                key: own.key.clone(),
                name: Some(own.name.clone().unwrap_or_else(|| own.key.clone())),
                built_in: false,
                entries: own.entries.clone().unwrap_or_default(),
                names: own.names,
                descriptions: own.descriptions,
                columns: own.columns.clone(),
                changed: false,
            });
        }
        groups
    }

    /// The group with `key` as the player sees it.
    #[must_use]
    pub fn group(&self, key: &str) -> Option<Group> {
        self.groups().into_iter().find(|group| group.key == key)
    }

    /// Changes the group `key`; a built-in group back as Harmonia ships it
    /// leaves nothing in the settings.
    pub fn change(&mut self, key: &str, change: impl FnOnce(&mut SavedGroup)) {
        let at = self
            .groups
            .iter()
            .position(|g| g.key == key)
            .unwrap_or_else(|| {
                self.groups.push(SavedGroup {
                    key: key.to_owned(),
                    ..SavedGroup::default()
                });
                self.groups.len() - 1
            });
        let saved = &mut self.groups[at];
        change(saved);
        if let Some(shipped) = built_in(key) {
            if saved.entries.as_ref().is_some_and(|entries| {
                entries
                    .iter()
                    .map(String::as_str)
                    .eq(shipped.entries.iter().copied())
            }) {
                saved.entries = None;
            }
            if saved
                .name
                .as_deref()
                .is_some_and(|name| name.trim().is_empty())
            {
                saved.name = None;
            }
            if *saved
                == (SavedGroup {
                    key: key.to_owned(),
                    ..SavedGroup::default()
                })
            {
                self.groups.remove(at);
            }
        }
    }

    /// Sets a kind of text of a group.
    pub fn choose(&mut self, key: &str, kind: Kind, keep: Keep) {
        self.change(key, |group| match kind {
            Kind::Name => group.names = keep,
            Kind::Description => group.descriptions = keep,
        });
    }

    /// Sets one column (`Item:description`) of a group; the group's own
    /// choice for the column's kind clears it.
    pub fn choose_column(&mut self, key: &str, column: &str, kind: Kind, keep: Keep) {
        self.change(key, |group| {
            let own = match kind {
                Kind::Name => group.names,
                Kind::Description => group.descriptions,
            };
            if own == keep {
                group.columns.remove(column);
            } else {
                group.columns.insert(column.to_owned(), keep);
            }
        });
    }

    /// Adds an entry to a group, once.
    pub fn add(&mut self, key: &str, entry: &str) {
        let Some(current) = self.group(key) else {
            return;
        };
        if current.entries.iter().any(|e| e == entry) {
            return;
        }
        let mut entries = current.entries;
        entries.push(entry.to_owned());
        self.change(key, |group| group.entries = Some(entries));
    }

    /// Takes an entry out of a group, and the columns set apart in it.
    pub fn take_out(&mut self, key: &str, entry: &str) {
        let Some(current) = self.group(key) else {
            return;
        };
        let entries: Vec<String> = current.entries.into_iter().filter(|e| e != entry).collect();
        let prefix = format!("{entry}:");
        self.change(key, |group| {
            group.entries = Some(entries);
            group
                .columns
                .retain(|column, _| !column.starts_with(&prefix) && column != entry);
        });
    }

    /// Renames a group; an empty name gives a built-in group its own back.
    pub fn rename(&mut self, key: &str, name: &str) {
        let name = name.trim();
        self.change(key, |group| {
            group.name = (!name.is_empty()).then(|| name.to_owned());
        });
    }

    /// A new group of the player's own; its key.
    pub fn new_group(&mut self, name: &str) -> String {
        let mut number = 1;
        while self.groups.iter().any(|g| g.key == format!("own{number}")) {
            number += 1;
        }
        let key = format!("own{number}");
        self.groups.push(SavedGroup {
            key: key.clone(),
            name: Some(name.trim().to_owned()),
            entries: Some(Vec::new()),
            ..SavedGroup::default()
        });
        key
    }

    /// Removes a group: a built-in one stays removed until restored.
    pub fn remove(&mut self, key: &str) {
        self.groups.retain(|group| group.key != key);
        if built_in(key).is_some() {
            self.groups.push(SavedGroup {
                key: key.to_owned(),
                removed: true,
                ..SavedGroup::default()
            });
        }
    }

    /// Gives a built-in group back its name, sheets, and columns as
    /// Harmonia ships them; its two choices stay.
    pub fn restore(&mut self, key: &str) {
        if built_in(key).is_none() {
            return;
        }
        self.change(key, |group| {
            group.name = None;
            group.entries = None;
            group.columns.clear();
            group.removed = false;
        });
    }

    /// The removed built-in groups, to bring back.
    #[must_use]
    pub fn removed(&self) -> Vec<&'static BuiltIn> {
        BUILT_IN
            .iter()
            .filter(|shipped| {
                self.groups
                    .iter()
                    .any(|g| g.key == shipped.key && g.removed)
            })
            .collect()
    }

    /// Whether some group keeps a column of `sheet` named `column` that
    /// holds `kind`.
    #[must_use]
    pub fn keeps(&self, sheet: &str, column: Option<&str>, kind: Kind) -> bool {
        self.groups()
            .iter()
            .any(|group| group.decides(sheet, column, kind) == Some(Keep::Original))
    }

    /// What decides the table: the groups that keep something, as text,
    /// empty when nothing is kept. A table built with another one is built
    /// again.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let kept: Vec<_> = self
            .groups()
            .into_iter()
            .filter(|group| {
                group.names == Keep::Original
                    || group.descriptions == Keep::Original
                    || group.columns.values().any(|keep| *keep == Keep::Original)
            })
            .map(|group| {
                (
                    group.entries,
                    group.names,
                    group.descriptions,
                    group.columns,
                )
            })
            .collect();
        // Nothing kept is empty, as in the tables of earlier versions.
        if kept.is_empty() {
            return String::new();
        }
        serde_json::to_string(&kept).unwrap_or_default()
    }

    /// Takes the sheets an earlier Harmonia kept whole into a group of
    /// their own.
    pub fn adopt(&mut self, kept_sheets: &[String], name: &str) {
        if kept_sheets.is_empty() {
            return;
        }
        let key = self.new_group(name);
        self.change(&key, |group| {
            group.entries = Some(kept_sheets.to_vec());
            group.names = Keep::Original;
            group.descriptions = Keep::Original;
        });
    }
}

/// What the table keeps of `sheet`, whose columns are `layout` (their
/// positions); `names` gives the name the map gives a column at a
/// position, and is asked only when the answer depends on it.
///
/// # Errors
/// What `names` returns.
pub fn sheet_keep(
    groups: &[Group],
    sheet: &str,
    layout: &[u16],
    names: impl FnOnce() -> Result<Vec<(String, u16)>, String>,
) -> Result<SheetKeep, String> {
    let touching: Vec<&Group> = groups.iter().filter(|g| g.has(sheet)).collect();
    if touching.is_empty() {
        return Ok(SheetKeep::Translate);
    }
    let by_column = touching.iter().any(|group| {
        group
            .entries
            .iter()
            .any(|e| matches!(covers(e, sheet), Covers::Column(_)))
            || group
                .columns
                .keys()
                .any(|c| c.starts_with(&format!("{sheet}:")))
    });
    let decide = |column: Option<&str>, kind| {
        touching
            .iter()
            .any(|group| group.decides(sheet, column, kind) == Some(Keep::Original))
    };
    let (names_kept, descriptions_kept) =
        (decide(None, Kind::Name), decide(None, Kind::Description));
    if !by_column && names_kept == descriptions_kept {
        return Ok(if names_kept {
            SheetKeep::Original
        } else {
            SheetKeep::Translate
        });
    }
    let given = names()?;
    let kept: Vec<u16> = layout
        .iter()
        .copied()
        .filter(|&index| {
            let column = given
                .iter()
                .find(|(_, at)| *at == index)
                .map(|(name, _)| name.as_str());
            decide(column, kind(sheet, column))
        })
        .collect();
    Ok(if kept.is_empty() {
        SheetKeep::Translate
    } else if kept.len() == layout.len() {
        SheetKeep::Original
    } else {
        SheetKeep::Columns(kept)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unnecessary_wraps)]
    fn columns() -> Result<Vec<(String, u16)>, String> {
        Ok(vec![("name".into(), 9), ("description".into(), 8)])
    }

    #[test]
    fn names_and_descriptions_are_kept_apart() {
        let mut coverage = Coverage::default();
        let groups = coverage.groups();
        assert_eq!(groups.len(), 10);
        assert_eq!(
            sheet_keep(&groups, "Item", &[0, 8, 9], columns),
            Ok(SheetKeep::Translate)
        );
        coverage.choose("items", Kind::Name, Keep::Original);
        let groups = coverage.groups();
        // The unnamed column 0 counts as a description of Item.
        assert_eq!(
            sheet_keep(&groups, "Item", &[0, 8, 9], columns),
            Ok(SheetKeep::Columns(vec![9]))
        );
        // EventItem is a sheet of names: its unnamed columns are names.
        assert_eq!(
            sheet_keep(&groups, "EventItem", &[0, 1], || Ok(Vec::new())),
            Ok(SheetKeep::Original)
        );
        assert_eq!(
            sheet_keep(&groups, "EventItemHelp", &[0], || Ok(Vec::new())),
            Ok(SheetKeep::Translate)
        );
        coverage.choose("items", Kind::Description, Keep::Original);
        let groups = coverage.groups();
        assert_eq!(
            sheet_keep(&groups, "Item", &[0, 8, 9], || Err("not asked".into())),
            Ok(SheetKeep::Original)
        );
    }

    #[test]
    fn a_column_is_set_apart_from_its_group() {
        let mut coverage = Coverage::default();
        coverage.choose_column(
            "items",
            "Item:description",
            Kind::Description,
            Keep::Original,
        );
        let groups = coverage.groups();
        assert_eq!(
            sheet_keep(&groups, "Item", &[0, 8, 9], columns),
            Ok(SheetKeep::Columns(vec![8]))
        );
        // Back to the group's choice, nothing is saved.
        coverage.choose_column(
            "items",
            "Item:description",
            Kind::Description,
            Keep::Translate,
        );
        assert!(coverage.groups.is_empty());
    }

    #[test]
    fn folders_and_column_entries() {
        let mut coverage = Coverage::default();
        coverage.choose("story", Kind::Description, Keep::Original);
        let key = coverage.new_group("Mine");
        coverage.add(&key, "Status:name");
        coverage.choose(&key, Kind::Name, Keep::Original);
        let groups = coverage.groups();
        assert_eq!(
            sheet_keep(&groups, "quest/000/Test", &[1], || Ok(Vec::new())),
            Ok(SheetKeep::Original)
        );
        assert_eq!(
            sheet_keep(&groups, "Quest", &[1], || Ok(Vec::new())),
            Ok(SheetKeep::Translate)
        );
        assert_eq!(
            sheet_keep(&groups, "Status", &[0, 1], || Ok(vec![("name".into(), 0)])),
            Ok(SheetKeep::Columns(vec![0]))
        );
    }

    #[test]
    fn groups_are_renamed_removed_and_restored() {
        let mut coverage = Coverage::default();
        coverage.rename("places", "Карта");
        coverage.take_out("places", "Town");
        coverage.choose("places", Kind::Name, Keep::Original);
        let places = coverage.group("places").unwrap();
        assert_eq!(places.title(true), "Карта");
        assert_eq!(places.entries, ["PlaceName", "Aetheryte"]);
        assert!(places.changed);
        coverage.restore("places");
        let places = coverage.group("places").unwrap();
        assert_eq!(places.title(true), "Места");
        assert_eq!(places.entries.len(), 3);
        assert_eq!(places.names, Keep::Original, "the choices stay");
        coverage.remove("places");
        assert!(coverage.group("places").is_none());
        assert_eq!(coverage.removed().len(), 1);
        coverage.restore("places");
        assert!(coverage.group("places").is_some());
        let own = coverage.new_group("Craft");
        coverage.remove(&own);
        assert!(coverage.group(&own).is_none());
    }

    #[test]
    fn fingerprint_follows_only_what_is_kept() {
        let mut coverage = Coverage::default();
        let empty = coverage.fingerprint();
        assert_eq!(empty, "");
        coverage.rename("items", "Вещи");
        assert_eq!(coverage.fingerprint(), empty);
        coverage.choose("items", Kind::Name, Keep::Original);
        assert_ne!(coverage.fingerprint(), empty);
    }

    #[test]
    fn kinds() {
        assert_eq!(kind("Item", Some("name")), Kind::Name);
        assert_eq!(kind("Item", None), Kind::Description);
        assert_eq!(kind("PlaceName", None), Kind::Name);
        assert_eq!(kind("Action", Some("description")), Kind::Description);
    }
}
