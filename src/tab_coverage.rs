//! The Coverage tab: what stays in the game's language. The groups on the
//! left, the chosen one on the right with its two choices, names and
//! descriptions, a sample of each, and its sheets; a sheet shows its
//! columns, each of which can be set apart.
use crate::{
    Harmonia, Side,
    catalog::{Catalog, Sheet},
    count,
    coverage::{self, Group, Keep, Kind},
};
use angelica_sdk::ui::{ActionKind, Dialog, Entry, Role, Split, Tone, Window};

/// Search results shown while adding sheets.
const FOUND: usize = 12;

fn kept(keep: Keep) -> bool {
    keep == Keep::Translate
}

fn keep(translated: bool) -> Keep {
    if translated {
        Keep::Translate
    } else {
        Keep::Original
    }
}

impl Harmonia {
    /// The translation whose sheets the tab shows.
    pub(crate) fn coverage_hash(&self) -> Option<String> {
        self.settings
            .active
            .clone()
            .or_else(|| self.applied.clone())
            .or_else(|| self.library.first().map(|t| t.hash.clone()))
    }

    pub(crate) fn coverage_group(&self) -> Option<Group> {
        let groups = self.settings.coverage.groups();
        self.coverage
            .group
            .as_deref()
            .filter(|key| !key.starts_with('-'))
            .and_then(|key| groups.iter().find(|g| g.key == key).cloned())
            .or_else(|| groups.into_iter().next())
    }

    fn catalog(&self) -> Option<&Catalog> {
        self.coverage.catalog.as_ref()
    }

    /// A sample of a kind of text of the group: `original → translation`.
    fn sample(&self, group: &Group, kind: Kind) -> Option<String> {
        let catalog = self.catalog()?;
        let mut fallback = None;
        for entry in &group.entries {
            for sheet in catalog.covered(entry) {
                for column in sheet.columns.iter().filter(|c| c.kind == kind) {
                    let Some(sample) = &column.sample else {
                        continue;
                    };
                    match &sample.original {
                        Some(original) => return Some(format!("{original} → {}", sample.text)),
                        None => {
                            fallback.get_or_insert_with(|| sample.text.clone());
                        }
                    }
                }
            }
        }
        fallback
    }

    /// The split's sections, top to bottom: the groups, then the removed
    /// built-in ones as `-<key>`.
    pub(crate) fn sections(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .settings
            .coverage
            .groups()
            .into_iter()
            .map(|g| g.key)
            .collect();
        keys.extend(
            self.settings
                .coverage
                .removed()
                .into_iter()
                .map(|b| format!("-{}", b.key)),
        );
        keys
    }

    /// The groups at the side: what each keeps in the game's language at
    /// its right (½ part of it, ○ all of it).
    fn group_split(&self, chosen: &str) -> Split {
        let keys = self.sections();
        #[allow(clippy::cast_possible_truncation)]
        let selected = keys.iter().position(|k| k == chosen).unwrap_or(0) as u32;
        let mut split = Split::new("groups", selected).width(190);
        let mut own = false;
        let mut removed = false;
        for group in self.settings.coverage.groups() {
            if !group.built_in && !own {
                own = true;
                split = split.group(self.tr("Mine", "Мои"));
            }
            let names = group.names == Keep::Original;
            let descriptions = group.descriptions == Keep::Original;
            let value = if names && descriptions {
                "○"
            } else if names || descriptions || group.columns.values().any(|k| *k == Keep::Original)
            {
                "½"
            } else {
                ""
            };
            split = split.section_valued(group.title(self.russian), value);
        }
        for shipped in self.settings.coverage.removed() {
            if !removed {
                removed = true;
                split = split.group(self.tr("Removed", "Удалённые"));
            }
            split = split.section(if self.russian { shipped.ru } else { shipped.en });
        }
        split.action(self.tr("+ Group", "+ Группа"))
    }

    /// What an entry is, for its row's note, and its rows.
    fn describe_entry(&self, entry: &str) -> (String, u32) {
        let Some(catalog) = self.catalog() else {
            return (String::new(), 0);
        };
        let sheets = catalog.covered(entry);
        let rows = sheets.iter().map(|s| s.rows).sum();
        if sheets.is_empty() {
            return (
                self.tr("not in this translation", "нет в этом переводе")
                    .to_owned(),
                0,
            );
        }
        if entry.ends_with('*') {
            return (
                if self.russian {
                    format!("папка · листов: {}", count(sheets.len() as u64, true))
                } else {
                    format!("folder · {} sheets", count(sheets.len() as u64, false))
                },
                rows,
            );
        }
        if let Some((_, column)) = entry.split_once(':') {
            return (
                if self.russian {
                    format!("столбец {column}")
                } else {
                    format!("column {column}")
                },
                rows,
            );
        }
        if let Some(quest) = catalog.quest(entry) {
            return (quest.shown.clone(), rows);
        }
        (String::new(), rows)
    }

    fn rows_text(&self, rows: u32) -> String {
        if self.russian {
            format!("строк: {}", count(rows, true))
        } else {
            format!("{} rows", count(rows, false))
        }
    }

    /// The columns of a sheet the map names, each with its own choice.
    fn columns(&self, mut column: Window, group: &Group, index: usize, sheet: &Sheet) -> Window {
        let mut unnamed = 0;
        for own in &sheet.columns {
            let Some(name) = &own.name else {
                unnamed += 1;
                continue;
            };
            let translated =
                group.decides(&sheet.name, Some(name), own.kind) != Some(Keep::Original);
            let what = match own.kind {
                Kind::Name => self.tr("names", "названия"),
                Kind::Description => self.tr("descriptions", "описания"),
            };
            column = column.checkbox_words(
                format!("col-{index}-{name}"),
                format!("{name} ({what})"),
                translated,
                self.tr("Translation", "Перевод"),
                self.tr("Original", "Оригинал"),
            );
            if let Some(sample) = &own.sample {
                column = column.muted(match &sample.original {
                    Some(original) => format!("{original} → {}", sample.text),
                    None => sample.text.clone(),
                });
            }
        }
        if unnamed > 0 {
            column = column.muted(if self.russian {
                format!(
                    "Столбцы без имени в карте игры ({unnamed}) идут с {} группы.",
                    match coverage::kind(&sheet.name, None) {
                        Kind::Name => "названиями",
                        Kind::Description => "описаниями",
                    }
                )
            } else {
                format!(
                    "Columns the game map does not name ({unnamed}) follow the group's {}.",
                    match coverage::kind(&sheet.name, None) {
                        Kind::Name => "names",
                        Kind::Description => "descriptions",
                    }
                )
            });
        }
        column
    }

    fn group_sheets(&self, mut column: Window, group: &Group) -> Window {
        column = column.heading(if self.russian {
            format!("Листы · {}", group.entries.len())
        } else {
            format!("Sheets · {}", group.entries.len())
        });
        if group.entries.is_empty() {
            column =
                column.muted(self.tr("No sheets in the group yet.", "В группе пока нет листов."));
        }
        for (index, entry) in group.entries.iter().enumerate() {
            let (note, rows) = self.describe_entry(entry);
            let chosen = self.coverage.entry.as_deref() == Some(entry.as_str());
            let mut row = Entry::new(format!("entry-{index}"), entry)
                .plain()
                .note(note)
                .chosen(chosen)
                .action(format!("out-{index}"), self.tr("Take out", "Убрать"));
            if rows > 0 {
                row = row.value(self.rows_text(rows));
            }
            if chosen {
                row = row.more(
                    format!("out-{index}"),
                    self.tr("Take out of the group", "Убрать из группы"),
                    ActionKind::Quiet,
                    true,
                );
            }
            column = column.entry(row);
            let sheet = (chosen && !entry.ends_with('*') && !entry.contains(':'))
                .then(|| self.catalog().and_then(|c| c.sheet(entry)))
                .flatten();
            if let Some(sheet) = sheet {
                column = self.columns(column, group, index, sheet);
            }
        }
        column
    }

    fn adding(&self, mut column: Window, group: &Group, query: &str) -> Window {
        column = column
            .heading(if self.russian {
                format!("Добавить в «{}»", group.title(true))
            } else {
                format!("Add to “{}”", group.title(false))
            })
            .input(
                "sheet-query",
                "",
                query,
                self.tr("A sheet, a folder, or a quest", "Лист, папка или квест"),
            );
        let query = query.trim();
        let found = self
            .coverage
            .found
            .as_ref()
            .filter(|(asked, _, _)| asked == query);
        if let (Some(catalog), Some((_, found, total))) = (self.catalog(), found) {
            column = column.muted(if self.russian {
                format!("Найдено: {}", count(*total as u64, true))
            } else {
                format!("Found: {}", count(*total as u64, false))
            });
            for (index, name) in found.iter().enumerate() {
                let (note, rows) = self.describe_entry(name);
                let sample = catalog.sheet(name).and_then(|sheet| {
                    sheet
                        .columns
                        .iter()
                        .find_map(|c| c.sample.as_ref())
                        .map(|s| match &s.original {
                            Some(original) => format!("{original} → {}", s.text),
                            None => s.text.clone(),
                        })
                });
                let note = match (note.is_empty(), sample) {
                    (true, Some(sample)) => sample,
                    (false, Some(sample)) => format!("{note} · {sample}"),
                    (_, None) => note,
                };
                let mut entry = Entry::new(format!("found-{index}"), name)
                    .plain()
                    .note(note);
                if group.entries.iter().any(|e| e == name) {
                    entry = entry.value(self.tr("in the group", "в группе"));
                } else {
                    entry = entry.value(self.rows_text(rows)).action(
                        format!("add-{index}"),
                        self.tr("+ To the group", "+ В группу"),
                    );
                }
                column = column.entry(entry);
            }
        }
        if !query.is_empty()
            && !group.entries.iter().any(|e| e == query)
            && found.is_none_or(|(_, found, _)| found.iter().all(|f| f != query))
        {
            column = column.action(
                "add-typed",
                if self.russian {
                    format!("+ Добавить «{query}»")
                } else {
                    format!("+ Add “{query}”")
                },
                ActionKind::Normal,
                true,
            );
        }
        column.row(|row| {
            row.action(
                "adding-done",
                self.tr("Done", "Готово"),
                ActionKind::Main,
                true,
            )
        })
    }

    fn group_detail(&self, mut column: Window, group: &Group) -> Window {
        if let Some(name) = &self.coverage.renaming {
            column = column
                .input(
                    "group-name",
                    "",
                    name,
                    self.tr("The group's name", "Название группы"),
                )
                .row(|row| {
                    row.action(
                        "rename-save",
                        self.tr("Save", "Сохранить"),
                        ActionKind::Main,
                        true,
                    )
                    .action(
                        "rename-cancel",
                        self.tr("Cancel", "Отмена"),
                        ActionKind::Quiet,
                        true,
                    )
                });
        }
        let (names, descriptions) = self
            .catalog()
            .map_or((true, true), |catalog| catalog.kinds(&group.entries));
        if names || group.entries.is_empty() {
            column = column.checkbox_words(
                "names",
                self.tr("Names", "Названия"),
                kept(group.names),
                self.tr("Translation", "Перевод"),
                self.tr("Original", "Оригинал"),
            );
            if let Some(sample) = self.sample(group, Kind::Name) {
                column = column.muted(sample);
            }
        }
        if descriptions || group.entries.is_empty() {
            column = column.checkbox_words(
                "descriptions",
                self.tr("Descriptions", "Описания"),
                kept(group.descriptions),
                self.tr("Translation", "Перевод"),
                self.tr("Original", "Оригинал"),
            );
            if let Some(sample) = self.sample(group, Kind::Description) {
                column = column.muted(sample);
            }
        }
        if let Some(query) = &self.coverage.adding {
            return self.adding(column, group, query);
        }
        column = self.group_sheets(column, group);
        column.row(|mut row| {
            row = row
                .action(
                    "add-sheets",
                    self.tr("+ Add sheets", "+ Добавить листы"),
                    ActionKind::Normal,
                    true,
                )
                .action(
                    "rename",
                    self.tr("Rename", "Переименовать"),
                    ActionKind::Quiet,
                    true,
                );
            if group.built_in && group.changed {
                row = row.action(
                    "restore",
                    self.tr("Restore", "Вернуть как было"),
                    ActionKind::Quiet,
                    true,
                );
            }
            row.action(
                "delete",
                self.tr("Delete", "Удалить"),
                ActionKind::Danger,
                true,
            )
        })
    }

    pub(crate) fn coverage_tab(&self, mut window: Window) -> Window {
        if self.library.is_empty() {
            window = window.muted(self.tr(
                "Install a translation to see its sheets and samples.",
                "Установите перевод, чтобы видеть его листы и примеры строк.",
            ));
        }
        let loading = self
            .side_running(&Side::Catalog(String::new()))
            .map(|fraction| fraction * 0.8)
            .or_else(|| {
                self.coverage
                    .loader
                    .as_ref()
                    .map(|l| 0.8 + 0.2 * l.fraction())
            });
        if let Some(fraction) = loading {
            window = window.progress(
                fraction,
                self.tr("Reading the translation's sheets", "Чтение листов перевода"),
            );
        }
        if let Some(reason) = &self.coverage.failed {
            window = window.toned(
                if self.russian {
                    format!("Листы перевода не прочитаны: {reason}")
                } else {
                    format!("The translation's sheets could not be read: {reason}")
                },
                Tone::Warning,
            );
        }
        if let Some(restart) = self.restart_line() {
            window = window.toned(restart, Tone::Warning);
        }
        let group = self.coverage_group();
        let chosen = self
            .coverage
            .group
            .clone()
            .filter(|key| key.starts_with('-'))
            .or_else(|| group.as_ref().map(|g| g.key.clone()))
            .unwrap_or_default();
        let split = self.group_split(&chosen);
        window.split(split, |side| {
            if let Some(key) = chosen.strip_prefix('-') {
                let name = coverage::BUILT_IN
                    .iter()
                    .find(|b| b.key == key)
                    .map_or(key, |b| if self.russian { b.ru } else { b.en });
                return side
                    .muted(if self.russian {
                        format!("Группа «{name}» удалена: её листы переводятся, если их не оставляет другая группа.")
                    } else {
                        format!("The group “{name}” is removed: its sheets are translated unless another group keeps them.")
                    })
                    .row(|row| {
                        row.action(
                            format!("bring-{key}"),
                            self.tr("Bring it back", "Вернуть"),
                            ActionKind::Main,
                            true,
                        )
                    });
            }
            match &group {
                Some(group) => self.group_detail(side, group),
                None => side,
            }
        })
    }

    /// The question whether to delete a group.
    pub(crate) fn group_dialog(&self) -> Option<Dialog> {
        let key = self.coverage.deleting.as_deref()?;
        let group = self.settings.coverage.group(key)?;
        Some(
            Dialog::new("delete-group", self.tr("Delete this group?", "Удалить группу?"))
                .styled(group.title(self.russian), Role::Title, Tone::Normal)
                .text(if group.built_in {
                    self.tr(
                        "Its sheets are translated again unless another group keeps them. It can be brought back below the groups.",
                        "Её листы снова переводятся, если их не оставляет другая группа. Вернуть её можно под списком групп.",
                    )
                } else {
                    self.tr(
                        "Its sheets are translated again unless another group keeps them.",
                        "Её листы снова переводятся, если их не оставляет другая группа.",
                    )
                })
                .row(|row| {
                    row.action("delete-yes", self.tr("Delete", "Удалить"), ActionKind::Danger, true)
                        .action("delete-no", self.tr("Cancel", "Отмена"), ActionKind::Quiet, true)
                }),
        )
    }

    /// Finds the sheets to add for the query typed, once per query.
    pub(crate) fn refresh_found(&mut self) {
        let Some(query) = self.coverage.adding.as_deref().map(str::trim) else {
            return;
        };
        if self
            .coverage
            .found
            .as_ref()
            .is_some_and(|(asked, _, _)| asked == query)
        {
            return;
        }
        let Some(catalog) = &self.coverage.catalog else {
            return;
        };
        let (found, total) = catalog.find(query, FOUND);
        self.coverage.found = Some((query.to_owned(), found, total));
    }

    /// A section of the split chosen.
    pub(crate) fn coverage_chosen(&mut self, index: u32) {
        if let Some(key) = self.sections().get(index as usize).cloned() {
            self.coverage.group = Some(key);
            self.coverage.entry = None;
            self.coverage.adding = None;
            self.coverage.renaming = None;
        }
    }

    /// Handles a click of the Coverage tab; false for a part it does not
    /// have.
    pub(crate) fn coverage_click(&mut self, id: &str) -> bool {
        let Some(group) = self.coverage_group() else {
            return false;
        };
        let key = group.key.clone();
        let index = |prefix: &str| {
            id.strip_prefix(prefix)
                .and_then(|i| i.parse::<usize>().ok())
        };
        if let Some(chosen) = id.strip_prefix("group-") {
            self.coverage.group = Some(chosen.to_owned());
            self.coverage.entry = None;
            self.coverage.adding = None;
            self.coverage.renaming = None;
        } else if let Some(shipped) = id.strip_prefix("bring-") {
            self.settings.coverage.restore(shipped);
            self.coverage.group = Some(shipped.to_owned());
            self.coverage_changed();
        } else if let Some(index) = index("entry-") {
            let entry = group.entries.get(index).cloned();
            self.coverage.entry = if self.coverage.entry == entry {
                None
            } else {
                entry
            };
        } else if let Some(index) = index("out-") {
            if let Some(entry) = group.entries.get(index) {
                self.settings.coverage.take_out(&key, entry);
                self.coverage.entry = None;
                self.coverage_changed();
            }
        } else if let Some(index) = index("add-") {
            if let Some(name) = self
                .coverage
                .found
                .as_ref()
                .and_then(|(_, found, _)| found.get(index).cloned())
            {
                self.settings.coverage.add(&key, &name);
                self.coverage_changed();
            }
        } else {
            match id {
                "new-group" | "groups" => {
                    let name = self.tr("New group", "Новая группа").to_owned();
                    let key = self.settings.coverage.new_group(&name);
                    self.coverage.group = Some(key);
                    self.coverage.renaming = Some(name);
                    self.coverage.adding = None;
                    self.save();
                }
                "add-typed" => {
                    let query = self.coverage.adding.clone().unwrap_or_default();
                    if !query.trim().is_empty() {
                        self.settings.coverage.add(&key, query.trim());
                        self.coverage_changed();
                    }
                }
                "add-sheets" => {
                    self.coverage.adding = Some(String::new());
                    self.coverage.renaming = None;
                }
                "adding-done" => self.coverage.adding = None,
                "rename" => {
                    self.coverage.renaming = Some(group.title(self.russian));
                    self.coverage.adding = None;
                }
                "rename-save" => {
                    if let Some(name) = self.coverage.renaming.take() {
                        let shipped = coverage::BUILT_IN.iter().find(|b| b.key == key);
                        // A built-in group's own name is no new name.
                        let name = if shipped
                            .is_some_and(|b| name.trim() == b.ru || name.trim() == b.en)
                        {
                            String::new()
                        } else {
                            name
                        };
                        self.settings.coverage.rename(&key, &name);
                        self.save();
                    }
                }
                "rename-cancel" => self.coverage.renaming = None,
                "restore" => {
                    self.settings.coverage.restore(&key);
                    self.coverage.entry = None;
                    self.coverage_changed();
                }
                "delete" => self.coverage.deleting = Some(key),
                "delete-yes" => {
                    if let Some(key) = self.coverage.deleting.take() {
                        self.settings.coverage.remove(&key);
                        self.coverage.group = None;
                        self.coverage.entry = None;
                        self.coverage_changed();
                    }
                }
                "delete-no" => self.coverage.deleting = None,
                _ => return false,
            }
        }
        true
    }

    pub(crate) fn coverage_toggled(&mut self, id: &str, checked: bool) {
        let Some(group) = self.coverage_group() else {
            return;
        };
        match id {
            "names" => self
                .settings
                .coverage
                .choose(&group.key, Kind::Name, keep(checked)),
            "descriptions" => {
                self.settings
                    .coverage
                    .choose(&group.key, Kind::Description, keep(checked));
            }
            _ => {
                let Some((index, column)) = id
                    .strip_prefix("col-")
                    .and_then(|rest| rest.split_once('-'))
                    .and_then(|(i, column)| Some((i.parse::<usize>().ok()?, column)))
                else {
                    return;
                };
                let Some(sheet) = group.entries.get(index) else {
                    return;
                };
                let kind = self
                    .catalog()
                    .and_then(|c| c.sheet(sheet))
                    .and_then(|s| s.columns.iter().find(|c| c.name.as_deref() == Some(column)))
                    .map_or_else(|| coverage::kind(sheet, Some(column)), |c| c.kind);
                self.settings.coverage.choose_column(
                    &group.key,
                    &format!("{sheet}:{column}"),
                    kind,
                    keep(checked),
                );
            }
        }
        self.coverage_changed();
    }
}
