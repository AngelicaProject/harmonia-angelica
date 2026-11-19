//! Sheet and column rules for text that stays in the game's language.
//! Groups are filters over those rules. Old group-based choices are preserved
//! when settings are first edited in the new Coverage tab.
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

fn part_key(sheet: &str, part: Kind) -> String {
    let label = match part {
        Kind::Name => "names",
        Kind::Description => "descriptions",
    };
    format!("{sheet}#{label}")
}

#[must_use]
pub fn entry_matches(entry: &str, sheet: &str) -> bool {
    let entry = entry.split_once(':').map_or(entry, |(sheet, _)| sheet);
    entry
        .strip_suffix('*')
        .map_or(entry == sheet, |folder| sheet.starts_with(folder))
}

/// Column indices used by the Dalamud Coverage view. Other columns remain
/// part of the whole sheet and are available through its checkbox.
#[must_use]
pub fn part_kind(sheet: &str, column: u16) -> Option<Kind> {
    use Kind::{Description, Name};
    Some(match (sheet, column) {
        ("Item", 0 | 2 | 9)
        | ("ContentRoulette", 0 | 1)
        | (
            "Status" | "CraftAction" | "GeneralAction" | "PetAction" | "BuddyAction"
            | "CompanyAction",
            0,
        )
        | ("CompanionTransient", 3)
        | ("Glasses", 4 | 6 | 13)
        | ("Achievement", 1)
        | (
            "EventItem"
            | "ContentFinderCondition"
            | "Action"
            | "Trait"
            | "Mount"
            | "Companion"
            | "Ornament"
            | "GlassesStyle"
            | "Title",
            _,
        ) => Name,
        ("Item", 8)
        | ("ContentRoulette", 3 | 4)
        | (
            "Status" | "CraftAction" | "GeneralAction" | "PetAction" | "BuddyAction"
            | "CompanyAction",
            1,
        )
        | ("CompanionTransient", 0 | 1 | 2 | 4)
        | ("Glasses", 12)
        | ("Achievement", 2)
        | (
            "EventItemHelp"
            | "ContentFinderConditionTransient"
            | "ActionTransient"
            | "TraitTransient"
            | "MountTransient"
            | "OrnamentTransient",
            _,
        ) => Description,
        _ => return None,
    })
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
    pub en_hint: &'static str,
    pub ru_hint: &'static str,
    pub entries: &'static [&'static str],
}

/// Names first, then descriptions and story text.
pub const BUILT_IN: [BuiltIn; 12] = [
    BuiltIn {
        key: "items",
        en: "Items",
        ru: "Предметы",
        en_hint: "Names and descriptions of items and key items.",
        ru_hint: "Названия и описания предметов и ключевых предметов.",
        entries: &["Item", "EventItem", "EventItemHelp"],
    },
    BuiltIn {
        key: "duties",
        en: "Duties",
        ru: "Подземелья, испытания и рейды",
        en_hint: "Dungeon, trial, and raid names and their Duty Finder descriptions.",
        ru_hint: "Названия и описания в поиске групп.",
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
        en_hint: "Zone, area, town, and aetheryte names.",
        ru_hint: "Названия зон, местностей, городов и эфиритов.",
        entries: &["PlaceName", "Town", "Aetheryte"],
    },
    BuiltIn {
        key: "actions",
        en: "Actions and statuses",
        ru: "Умения и эффекты",
        en_hint: "Actions, traits, statuses, crafting and general actions, with their descriptions.",
        ru_hint: "Умения, свойства, эффекты, ремесленные и общие действия с их описаниями.",
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
        en_hint: "Class and job names.",
        ru_hint: "Названия классов и профессий.",
        entries: &["ClassJob", "ClassJobCategory"],
    },
    BuiltIn {
        key: "characters",
        en: "Character names",
        ru: "Имена персонажей",
        en_hint: "Names of NPCs and enemies.",
        ru_hint: "Имена NPC и противников.",
        entries: &["ENpcResident", "BNpcName"],
    },
    BuiltIn {
        key: "collections",
        en: "Mounts, minions, and accessories",
        ru: "Ездовые животные, компаньоны и аксессуары",
        en_hint: "Names and descriptions of mounts, minions, fashion accessories, and glasses.",
        ru_hint: "Названия и описания ездовых животных, компаньонов, аксессуаров и очков.",
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
        en_hint: "Achievement names and descriptions, and titles.",
        ru_hint: "Названия и описания достижений, титулы.",
        entries: &["Achievement", "Title"],
    },
    BuiltIn {
        key: "quest_names",
        en: "Quest names",
        ru: "Названия квестов",
        en_hint: "Quest titles in the journal and the quest list.",
        ru_hint: "Названия квестов в журнале и списке заданий.",
        entries: &["Quest"],
    },
    BuiltIn {
        key: "story",
        en: "Quest text",
        ru: "Тексты квестов",
        en_hint: "Dialogue and journal text of quests.",
        ru_hint: "Диалоги и тексты журнала квестов.",
        entries: &["quest/*"],
    },
    BuiltIn {
        key: "cutscenes",
        en: "Cutscenes",
        ru: "Сюжетные сцены",
        en_hint: "Cutscene subtitles.",
        ru_hint: "Субтитры сюжетных сцен.",
        entries: &["cut_scene/*"],
    },
    BuiltIn {
        key: "custom",
        en: "NPC conversations and menus",
        ru: "Разговоры с NPC и меню",
        en_hint: "Talk with NPCs outside quests, with the choices of their menus (the summoning bell and the like).",
        ru_hint: "Разговоры с NPC вне квестов вместе с пунктами их меню (колокольчик вызова и т. п.).",
        entries: &["custom/*"],
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy: Option<Vec<SavedGroup>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    rules: BTreeMap<String, Keep>,
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
    #[must_use]
    pub fn hint(&self, russian: bool) -> Option<&'static str> {
        let group = built_in(&self.key)?;
        Some(if russian {
            group.ru_hint
        } else {
            group.en_hint
        })
    }

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

impl SheetKeep {
    #[must_use]
    pub fn keeps(&self, column: u16) -> bool {
        match self {
            Self::Translate => false,
            Self::Original => true,
            Self::Columns(columns) => columns.contains(&column),
        }
    }
}

impl Coverage {
    /// Freezes the effective choices of an older installation before groups become filters.
    pub fn separate_groups(&mut self) {
        if self.legacy.is_some() {
            return;
        }
        let saved = self.policy_groups();
        self.legacy = Some(
            saved
                .into_iter()
                .filter(|group| {
                    group.names == Keep::Original
                        || group.descriptions == Keep::Original
                        || group.columns.values().any(|keep| *keep == Keep::Original)
                })
                .map(|group| SavedGroup {
                    key: group.key,
                    entries: Some(group.entries),
                    names: group.names,
                    descriptions: group.descriptions,
                    columns: group.columns,
                    ..SavedGroup::default()
                })
                .collect(),
        );
    }

    #[must_use]
    pub fn policy_groups(&self) -> Vec<Group> {
        if let Some(legacy) = &self.legacy {
            return legacy
                .iter()
                .map(|group| Group {
                    key: group.key.clone(),
                    name: None,
                    built_in: false,
                    entries: group.entries.clone().unwrap_or_default(),
                    names: group.names,
                    descriptions: group.descriptions,
                    columns: group.columns.clone(),
                    changed: false,
                })
                .collect();
        }
        let mut groups = self.groups();
        // Older versions included both folders in the built-in story group.
        if self
            .groups
            .iter()
            .find(|g| g.key == "story")
            .is_none_or(|g| g.entries.is_none())
            && let Some(story) = groups.iter_mut().find(|g| g.key == "story")
        {
            story
                .entries
                .extend(["cut_scene/*".to_owned(), "custom/*".to_owned()]);
        }
        groups
    }

    pub fn mark_many(&mut self, entries: &[String], part: Option<Kind>, keep: Keep) {
        self.separate_groups();
        if let Some(part) = part {
            for entry in entries {
                if keep == Keep::Translate && self.rules.get(entry) == Some(&Keep::Original) {
                    // Taking one part out of an explicitly kept sheet leaves
                    // only its other part in the game's language.
                    let other = match part {
                        Kind::Name => Kind::Description,
                        Kind::Description => Kind::Name,
                    };
                    self.rules.insert(entry.clone(), Keep::Translate);
                    self.rules.insert(part_key(entry, other), Keep::Original);
                }
                self.rules.insert(part_key(entry, part), keep);
            }
        } else {
            let targets: std::collections::BTreeSet<_> =
                entries.iter().map(String::as_str).collect();
            self.rules.retain(|key, _| {
                let sheet = key.split_once('#').map_or(key.as_str(), |(sheet, _)| sheet);
                !targets.contains(sheet)
                    && !sheet
                        .match_indices('/')
                        .any(|(at, _)| targets.contains(format!("{}*", &sheet[..=at]).as_str()))
            });
            self.rules
                .extend(entries.iter().cloned().map(|entry| (entry, keep)));
        }
    }

    /// # Errors
    /// When resolving the named columns for legacy coverage fails.
    pub fn effective_sheet(
        &self,
        groups: &[Group],
        sheet: &str,
        layout: &[u16],
        names: impl FnOnce() -> Result<Vec<(String, u16)>, String>,
    ) -> Result<SheetKeep, String> {
        let whole = self
            .rules
            .get(sheet)
            .or_else(|| {
                sheet
                    .rmatch_indices('/')
                    .find_map(|(at, _)| self.rules.get(&format!("{}*", &sheet[..=at])))
            })
            .copied();
        let names_rule = self.rules.get(&part_key(sheet, Kind::Name)).copied();
        let descriptions_rule = self.rules.get(&part_key(sheet, Kind::Description)).copied();
        let fallback = match whole {
            Some(Keep::Original) => SheetKeep::Original,
            Some(Keep::Translate) => SheetKeep::Translate,
            None => sheet_keep(groups, sheet, layout, names)?,
        };
        if names_rule.is_none() && descriptions_rule.is_none() {
            return Ok(fallback);
        }
        let kept: Vec<_> = layout
            .iter()
            .copied()
            .filter(|&column| {
                let rule = match part_kind(sheet, column) {
                    Some(Kind::Name) => names_rule,
                    Some(Kind::Description) => descriptions_rule,
                    None => None,
                };
                rule.map_or_else(|| fallback.keeps(column), |keep| keep == Keep::Original)
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
        self.coverage_fingerprint(false)
    }

    /// The key used by earlier builds to stamp their compiled tables.
    #[must_use]
    pub fn previous_fingerprint(&self) -> String {
        self.coverage_fingerprint(true)
    }

    fn coverage_fingerprint(&self, include_redundant_rules: bool) -> String {
        let kept: Vec<_> = self
            .policy_groups()
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
        let rules: BTreeMap<_, _> = self
            .rules
            .iter()
            .filter(|&(key, keep)| {
                if include_redundant_rules {
                    return true;
                }
                let (entry, part) = key
                    .split_once('#')
                    .map_or((key.as_str(), false), |(entry, _)| (entry, true));
                let path = entry.strip_suffix("/*").unwrap_or(entry);
                let inherited = part.then(|| self.rules.get(entry)).flatten().or_else(|| {
                    path.rmatch_indices('/')
                        .find_map(|(at, _)| self.rules.get(&format!("{}*", &path[..=at])))
                });
                if let Some(inherited) = inherited {
                    return inherited != keep;
                }
                // An explicit translation is needed where a legacy rule can keep text.
                *keep == Keep::Original
                    || kept.iter().any(|(entries, _, _, _)| {
                        entries.iter().any(|legacy| {
                            let legacy = legacy.split(':').next().unwrap_or(legacy);
                            entry_matches(legacy, entry) || entry_matches(entry, legacy)
                        })
                    })
            })
            .collect();
        if !rules.is_empty() {
            return serde_json::to_string(&(kept, rules)).unwrap_or_default();
        }
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

    #[test]
    fn folder_scope_and_parts_can_override_older_group_marks_independently() {
        let mut coverage = Coverage::default();
        coverage.choose("story", Kind::Description, Keep::Original);
        coverage.separate_groups();
        coverage.remove("story");
        let groups = coverage.policy_groups();
        let effective = |coverage: &Coverage, sheet, columns: &[u16]| {
            coverage
                .effective_sheet(&groups, sheet, columns, || Ok(vec![]))
                .unwrap()
        };
        assert_eq!(
            effective(&coverage, "cut_scene/example", &[1]),
            SheetKeep::Original
        );
        coverage.mark_many(&["quest/*".into()], None, Keep::Original);
        coverage.mark_many(&["quest/001/One".into()], None, Keep::Translate);
        assert_eq!(
            effective(&coverage, "quest/001/One", &[1]),
            SheetKeep::Translate
        );
        assert_eq!(
            effective(&coverage, "quest/001/Two", &[1]),
            SheetKeep::Original
        );
        coverage.mark_many(&["Item".into()], Some(Kind::Name), Keep::Original);
        assert_eq!(
            effective(&coverage, "Item", &[0, 2, 8, 9]),
            SheetKeep::Columns(vec![0, 2, 9])
        );
        coverage.mark_many(&["Item".into()], None, Keep::Original);
        coverage.mark_many(&["Item".into()], Some(Kind::Name), Keep::Translate);
        assert_eq!(
            effective(&coverage, "Item", &[0, 2, 8, 9, 12]),
            SheetKeep::Columns(vec![8])
        );
    }

    #[allow(clippy::unnecessary_wraps)]
    fn columns() -> Result<Vec<(String, u16)>, String> {
        Ok(vec![("name".into(), 9), ("description".into(), 8)])
    }

    #[test]
    fn names_and_descriptions_are_kept_apart() {
        let mut coverage = Coverage::default();
        let groups = coverage.groups();
        assert_eq!(groups.len(), 12);
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
    fn undoing_a_mark_restores_the_applied_fingerprint() {
        let mut coverage = Coverage::default();
        let original = coverage.fingerprint();
        for (entry, part) in [("Item", Some(Kind::Name)), ("quest/*", None)] {
            let target = [entry.to_owned()];
            coverage.mark_many(&target, part, Keep::Original);
            assert_ne!(coverage.fingerprint(), original);
            coverage.mark_many(&target, part, Keep::Translate);
            assert_eq!(coverage.fingerprint(), original);
        }

        coverage.mark_many(&["quest/*".into()], None, Keep::Original);
        let marked_folder = coverage.fingerprint();
        let child = ["quest/001/One".into()];
        coverage.mark_many(&child, None, Keep::Translate);
        assert_ne!(coverage.fingerprint(), marked_folder);
        coverage.mark_many(&child, None, Keep::Original);
        assert_eq!(coverage.fingerprint(), marked_folder);
    }

    #[test]
    fn kinds() {
        assert_eq!(kind("Item", Some("name")), Kind::Name);
        assert_eq!(kind("Item", None), Kind::Description);
        assert_eq!(kind("PlaceName", None), Kind::Name);
        assert_eq!(kind("Action", Some("description")), Kind::Description);
    }
}
