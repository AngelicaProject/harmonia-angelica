use crate::{
    Harmonia, Side,
    coverage::{Group, Keep, Kind},
    coverage_browser::{Expansion, Request, Row, State, View},
};
use angelica_sdk::ui::{
    ActionKind, CheckState, Dialog, Entry, Role, Split, Tone, VirtualList, Window,
};
use std::fmt::Write;

fn row_index(id: &str, prefix: &str) -> Option<usize> {
    id.strip_prefix(prefix)?.parse().ok()
}

fn checkbox_state(state: State) -> CheckState {
    match state {
        State::None => CheckState::Unchecked,
        State::Some => CheckState::Mixed,
        State::All => CheckState::Checked,
    }
}

impl Harmonia {
    pub(crate) fn coverage_hash(&self) -> Option<String> {
        self.settings
            .active
            .clone()
            .or_else(|| self.applied.clone())
            .or_else(|| self.library.first().map(|pack| pack.hash.clone()))
    }

    pub(crate) fn coverage_group(&self) -> Option<Group> {
        self.settings
            .coverage
            .group(self.coverage.group.as_deref()?)
    }

    pub(crate) fn invalidate_coverage_view(&mut self) {
        self.coverage.revision = self.coverage.revision.wrapping_add(1);
        self.coverage.browse_error = None;
    }

    pub(crate) fn refresh_coverage_view(&mut self) {
        if !self.open || self.tab != crate::Tab::Coverage {
            return;
        }
        let Some(hash) = self.coverage_hash() else {
            return;
        };
        if self
            .coverage
            .catalog
            .as_ref()
            .is_none_or(|catalog| catalog.hash != hash)
        {
            if self.coverage.browser.take().is_some() {
                self.invalidate_coverage_view();
            }
            self.want_catalog(&hash);
            return;
        }
        if self.coverage.browser_revision == Some(self.coverage.revision)
            || self.coverage.browse_error.is_some()
            || self.side_running(&Side::Browse(0)).is_some()
        {
            return;
        }
        let input = Request {
            hash,
            scope: self.coverage_group().map(|group| group.entries),
            groups: self
                .settings
                .coverage
                .groups()
                .into_iter()
                .map(|group| (group.key, group.entries))
                .collect(),
            coverage: self.settings.coverage.clone(),
            filter: self.coverage.filter.clone(),
            adding: self.coverage.adding.clone(),
        };
        self.start_side(
            "browse-coverage",
            &input,
            Side::Browse(self.coverage.revision),
        );
    }

    pub(crate) fn coverage_tab(&self, window: Window) -> Window {
        let group = self.coverage_group();
        let groups = self.settings.coverage.groups();
        let selected = groups
            .iter()
            .position(|item| group.as_ref().is_some_and(|g| g.key == item.key))
            .and_then(|index| u32::try_from(index + 1).ok())
            .unwrap_or(0);
        let mut split = Split::new("groups", selected)
            .width(210)
            .section(self.tr("All sheets", "Все листы"))
            .group(self.tr("Groups", "Группы"));
        for item in &groups {
            let label = item.title(self.russian);
            split = if item.entries.is_empty() {
                split.section(label)
            } else {
                let state = self
                    .coverage
                    .browser
                    .as_ref()
                    .and_then(|view| view.group_marks.get(&item.key))
                    .copied()
                    .unwrap_or_default();
                split.section_checked(label, checkbox_state(state))
            };
        }
        window.split(
            split.action(self.tr("+ New group", "+ Новая группа")),
            |side| {
                let side = group.as_ref().map_or_else(
                    || {
                        side.clone().styled(
                            self.tr("All sheets", "Все листы"),
                            Role::Title,
                            Tone::Normal,
                        )
                    },
                    |group| self.group_header(side.clone(), group),
                );
                if self.coverage.renaming.is_some() {
                    return side;
                }
                if self.coverage.adding.is_some() {
                    let view =
                        self.coverage.browser.as_ref().filter(|_| {
                            self.coverage.browser_revision == Some(self.coverage.revision)
                        });
                    return self.group_picker(side, view);
                }
                if self.coverage_hash().is_none() {
                    let side = side.muted(self.tr(
                        "Install a translation to see its sheets.",
                        "Установите перевод, чтобы увидеть его листы.",
                    ));
                    return group.as_ref().map_or_else(
                        || side.clone(),
                        |group| self.group_entries(side.clone(), group),
                    );
                }
                if group.as_ref().is_some_and(|group| group.entries.is_empty()) {
                    return side.muted(
                        self.tr("Add sheets to this group.", "Добавьте листы в эту группу."),
                    );
                }
                if let Some(error) = self
                    .coverage
                    .failed
                    .as_ref()
                    .or(self.coverage.browse_error.as_ref())
                {
                    return side.toned(error, Tone::Warning);
                }
                self.sheet_tree(side)
            },
        )
    }

    fn group_header(&self, window: Window, group: &Group) -> Window {
        if let Some(rename) = &self.coverage.renaming {
            return window
                .input(
                    "group-name",
                    "",
                    rename,
                    self.tr("Group name", "Название группы"),
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
        let mut window = window.styled(group.title(self.russian), Role::Title, Tone::Normal);
        if !group.changed
            && let Some(hint) = group.hint(self.russian)
        {
            window = window.tooltip(hint);
        }
        if self.coverage.adding.is_some() {
            return window;
        }
        window = window.row(|row| {
            row.action(
                "add-sheets",
                self.tr("+ Add sheets", "+ Добавить листы"),
                ActionKind::Normal,
                true,
            )
            .action(
                "group-actions",
                self.tr("Manage group", "Управление группой"),
                ActionKind::Quiet,
                true,
            )
        });
        if !self.coverage.group_actions {
            return window;
        }
        window.row(|row| {
            let row = row.action(
                "rename",
                self.tr("Rename", "Переименовать"),
                ActionKind::Quiet,
                true,
            );
            let row = if group.built_in && group.changed {
                row.action(
                    "restore",
                    self.tr("Reset", "Сбросить"),
                    ActionKind::Quiet,
                    true,
                )
            } else {
                row
            };
            row.action(
                "delete",
                self.tr("Delete group", "Удалить группу"),
                ActionKind::Danger,
                true,
            )
        })
    }

    fn group_entries(&self, mut window: Window, group: &Group) -> Window {
        for (index, entry) in group.entries.iter().enumerate() {
            window = window.entry(
                Entry::new(format!("membership-{index}"), entry)
                    .plain()
                    .action(format!("remove-entry-{index}"), self.tr("Remove", "Убрать")),
            );
        }
        window
    }

    fn group_picker(&self, mut window: Window, view: Option<&View>) -> Window {
        let query = self.coverage.adding.as_deref().unwrap_or("").trim();
        window = window.input(
            "sheet-query",
            "",
            self.coverage.adding.as_deref().unwrap_or(""),
            self.tr("Find a sheet or folder", "Поиск по имени или папке"),
        );
        if query.is_empty() {
            window = window.muted(self.tr(
                "Enter a sheet or folder name, such as Item or quest/*.",
                "Введите имя листа или папку, например Item или quest/*.",
            ));
        } else if let Some(view) = view.filter(|view| !view.candidates.is_empty()) {
            for (index, candidate) in view.candidates.iter().enumerate() {
                window = window.entry(
                    Entry::new(format!("candidate-{index}"), candidate)
                        .plain()
                        .action(format!("add-{index}"), self.tr("Add", "Добавить")),
                );
            }
            if view.candidate_count > view.candidates.len() {
                window = window.muted(self.tr(
                    "Refine your search to see more.",
                    "Уточните поиск, чтобы увидеть остальные листы.",
                ));
            }
        } else if view.is_some() || self.coverage_hash().is_none() {
            window = window
                .row(|row| row.action("add-typed", format!("+ {query}"), ActionKind::Normal, true));
        }
        window.row(|row| {
            row.action(
                "adding-done",
                self.tr("Done", "Готово"),
                ActionKind::Quiet,
                true,
            )
        })
    }

    fn sheet_tree(&self, mut window: Window) -> Window {
        let filter = &self.coverage.filter;
        let filtered = filter.part.is_some() || filter.marked.is_some();
        window = window.input(
            "coverage-query",
            "",
            &filter.query,
            self.tr(
                "Find a sheet, folder or quest",
                "Найти лист, папку или квест",
            ),
        );
        let view = self.coverage.browser.as_ref();
        window = window.row(|row| {
            let row = row.action(
                "coverage-filters",
                if filtered {
                    self.tr("Filters active", "Фильтры включены")
                } else {
                    self.tr("Filters", "Фильтры")
                },
                if filtered {
                    ActionKind::Normal
                } else {
                    ActionKind::Quiet
                },
                true,
            );
            if !filter.query.is_empty() {
                row.action(
                    "coverage-clear",
                    self.tr("Clear search", "Очистить поиск"),
                    ActionKind::Quiet,
                    true,
                )
            } else if !filter.open.is_empty() {
                row.action(
                    "coverage-collapse",
                    self.tr("Collapse folders", "Свернуть папки"),
                    ActionKind::Quiet,
                    true,
                )
            } else {
                row
            }
        });
        if self.coverage.filters_open {
            window = self.coverage_filter_options(window, view);
        }
        let Some(view) = view else {
            return window.muted(self.tr("Loading sheets...", "Загрузка листов..."));
        };
        if view.rows.is_empty() {
            window = window.muted(self.tr("No matching sheets.", "Листы не найдены."));
        } else {
            window = window
                .muted(self.tr(
                    "Check a box to keep that text in the game's language. Changes apply after restarting the game.",
                    "Поставьте галочку, чтобы оставить текст на языке игры. Изменения вступят в силу после перезапуска.",
                ))
                .virtual_list(
                    VirtualList {
                        id: "coverage-list".into(),
                        first: u32::try_from(view.first).unwrap_or_default(),
                        total: u32::try_from(view.total_rows).unwrap_or_default(),
                        epoch: self.coverage.scroll_epoch,
                    },
                    |mut list| {
                        for (index, row) in view.rows.iter().enumerate() {
                            list = self.sheet_row(list, row, index);
                        }
                        list
                    },
                );
        }
        if !view.missing.is_empty() {
            window = window.section(
                "coverage-missing",
                self.tr("Missing from this translation", "Нет в этом переводе"),
                false,
                |mut list| {
                    for (index, entry) in view.missing.iter().enumerate() {
                        list = list.entry(
                            Entry::new(format!("missing-{index}"), entry)
                                .plain()
                                .action(
                                    format!("remove-missing-{index}"),
                                    self.tr("Remove", "Убрать"),
                                ),
                        );
                    }
                    list
                },
            );
        }
        window
    }

    fn coverage_filter_options(&self, mut window: Window, view: Option<&View>) -> Window {
        let filter = &self.coverage.filter;
        window = window.row(|row| {
            row.choice(
                "coverage-part",
                "",
                [
                    self.tr("Names and descriptions", "Названия и описания"),
                    self.tr("Names only", "Только названия"),
                    self.tr("Descriptions only", "Только описания"),
                ]
                .map(str::to_owned)
                .to_vec(),
                Some(match filter.part {
                    None => 0,
                    Some(Kind::Name) => 1,
                    Some(Kind::Description) => 2,
                }),
            )
            .choice(
                "coverage-show",
                "",
                [
                    self.tr("Any state", "Любой статус"),
                    self.tr("Kept original", "Оставлены в оригинале"),
                    self.tr("Translated", "Переводятся"),
                ]
                .map(str::to_owned)
                .to_vec(),
                Some(match filter.marked {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                }),
            )
        });
        if let Some(view) = view.filter(|view| !view.targets.is_empty()) {
            window = window
                .action(
                    "mark-shown",
                    if view.all_marked {
                        self.tr("Translate all results", "Перевести всё найденное")
                    } else {
                        self.tr(
                            "Keep all results original",
                            "Оставить всё найденное в оригинале",
                        )
                    },
                    ActionKind::Quiet,
                    true,
                )
                .tooltip(self.tr(
                    "Includes sheets inside collapsed folders.",
                    "Включая листы внутри свёрнутых папок.",
                ));
        }
        window
    }

    fn sheet_row(&self, window: Window, row: &Row, index: usize) -> Window {
        let label = match row.part {
            Some(Kind::Name) => self.tr("Names", "Названия"),
            Some(Kind::Description) => self.tr("Descriptions", "Описания"),
            None => row.quest.as_deref().unwrap_or(&row.title),
        };
        window.row(|line| {
            let line = if row.depth == 0 {
                line
            } else {
                line.text("\u{2003}".repeat(row.depth.min(5)))
            };
            let line = line
                .tick_box(format!("sheet-mark-{index}"), checkbox_state(row.state))
                .tooltip(if row.state == State::All {
                    self.tr("Translate", "Перевести")
                } else {
                    self.tr("Keep the original text", "Оставить текст на языке игры")
                });
            let title = match row.expansion {
                Expansion::Leaf => label.to_owned(),
                Expansion::Collapsed => format!("▸ {label}"),
                Expansion::Expanded | Expansion::Forced => format!("▾ {label}"),
            };
            let line = if row.expansion.can_toggle() {
                line.action(format!("sheet-row-{index}"), title, ActionKind::Quiet, true)
            } else {
                line.text(title)
            };
            let line = if row.quest.is_some() {
                line.tooltip(&row.path)
            } else {
                line
            };
            if row.part.is_none() {
                line.spacer()
                    .action(
                        format!("sheet-details-{index}"),
                        "...",
                        ActionKind::Quiet,
                        true,
                    )
                    .tooltip(self.tr("Sheet details and groups", "Сведения и группы"))
            } else {
                line
            }
        })
    }

    pub(crate) fn group_dialog(&self) -> Option<Dialog> {
        if let Some(row) = &self.coverage.details {
            let mut dialog = Dialog::new(
                "coverage-details",
                row.quest.as_deref().unwrap_or(&row.title),
            );
            if row.path != row.title || row.quest.is_some() {
                dialog = dialog.muted(&row.path);
            }
            for sample in &row.samples {
                dialog = dialog.text(sample.original.as_ref().map_or_else(
                    || sample.text.clone(),
                    |original| format!("{original} → {}", sample.text),
                ));
            }
            return Some(dialog.row(|line| {
                let line = line.action(
                    "details-groups",
                    self.tr("Manage groups", "Управление группами"),
                    ActionKind::Normal,
                    true,
                );
                let line = if row.folder {
                    line
                } else {
                    line.action("details-link", "XIVViewer", ActionKind::Quiet, true)
                };
                line.action(
                    "details-done",
                    self.tr("Done", "Готово"),
                    ActionKind::Quiet,
                    true,
                )
            }));
        }
        if let Some(entry) = &self.coverage.membership {
            let mut dialog =
                Dialog::new("coverage-groups", self.tr("Sheet groups", "Группы листа")).text(entry);
            for group in self.settings.coverage.groups() {
                let state = if group.entries.contains(entry) {
                    CheckState::Checked
                } else {
                    CheckState::Unchecked
                };
                dialog = dialog.tick_box_labeled(
                    format!("member-{}", group.key),
                    group.title(self.russian),
                    state,
                );
            }
            return Some(dialog.row(|row| {
                row.action(
                    "group-with-sheet",
                    self.tr("New group with this sheet", "Новая группа с этим листом"),
                    ActionKind::Normal,
                    true,
                )
                .action(
                    "membership-done",
                    self.tr("Done", "Готово"),
                    ActionKind::Quiet,
                    true,
                )
            }));
        }
        let group = self
            .settings
            .coverage
            .group(self.coverage.deleting.as_deref()?)?;
        Some(
            Dialog::new("delete-group", self.tr("Delete group?", "Удалить группу?"))
                .styled(group.title(self.russian), Role::Title, Tone::Normal)
                .text(self.tr(
                    "Its sheets keep their current marks.",
                    "Отметки на листах группы сохранятся.",
                ))
                .row(|row| {
                    row.action(
                        "delete-yes",
                        self.tr("Delete", "Удалить"),
                        ActionKind::Danger,
                        true,
                    )
                    .action(
                        "delete-no",
                        self.tr("Cancel", "Отмена"),
                        ActionKind::Quiet,
                        true,
                    )
                }),
        )
    }

    fn refresh_tree(&mut self) {
        self.coverage.filter.first = 0;
        self.coverage.scroll_epoch = self.coverage.scroll_epoch.wrapping_add(1);
        self.invalidate_coverage_view();
    }

    pub(crate) fn coverage_chosen(&mut self, id: &str, index: u32) -> bool {
        match id {
            "coverage-list" => {
                let first = index as usize;
                if self.coverage.filter.first != first {
                    self.coverage.filter.first = first;
                    self.invalidate_coverage_view();
                }
                return true;
            }
            "groups" => {
                self.coverage.group_actions = false;
                self.coverage.group = index.checked_sub(1).and_then(|index| {
                    self.settings
                        .coverage
                        .groups()
                        .get(index as usize)
                        .map(|g| g.key.clone())
                });
                self.coverage.adding = None;
                self.coverage.renaming = None;
            }
            "coverage-part" => {
                self.coverage.filter.part = [None, Some(Kind::Name), Some(Kind::Description)]
                    .get(index as usize)
                    .copied()
                    .flatten();
            }
            "coverage-show" => {
                self.coverage.filter.marked = [None, Some(true), Some(false)]
                    .get(index as usize)
                    .copied()
                    .flatten();
            }
            _ => return false,
        }
        self.refresh_tree();
        true
    }

    pub(crate) fn coverage_edited(&mut self, id: &str, value: String) {
        match id {
            "sheet-query" => self.coverage.adding = Some(value),
            "coverage-query" => self.coverage.filter.query = value,
            "group-name" => {
                self.coverage.renaming = Some(value);
                return;
            }
            _ => return,
        }
        self.refresh_tree();
    }

    fn new_coverage_group(&mut self, entry: Option<String>) {
        self.settings.coverage.separate_groups();
        let base = self.tr("New group", "Новая группа");
        let groups = self.settings.coverage.groups();
        let name = (1..=groups.len() + 1)
            .map(|n| {
                if n == 1 {
                    base.to_owned()
                } else {
                    format!("{base} {n}")
                }
            })
            .find(|name| {
                !groups
                    .iter()
                    .any(|group| group.title(self.russian).to_lowercase() == name.to_lowercase())
            })
            .unwrap_or_else(|| base.to_owned());
        let key = self.settings.coverage.new_group(&name);
        if let Some(entry) = entry {
            self.settings.coverage.add(&key, &entry);
        }
        self.coverage.group = Some(key);
        self.coverage.group_actions = false;
        self.coverage.renaming = Some(name);
        self.coverage.adding = None;
        self.save();
        self.refresh_tree();
    }

    fn apply_marks(&mut self, targets: &[String], part: Option<Kind>, choice: Keep) {
        if targets.is_empty() {
            return;
        }
        self.settings.coverage.mark_many(targets, part, choice);
        self.coverage_changed();
        self.invalidate_coverage_view();
    }

    pub(crate) fn coverage_membership_toggled(&mut self, id: &str, checked: bool) {
        let (Some(key), Some(entry)) = (id.strip_prefix("member-"), &self.coverage.membership)
        else {
            return;
        };
        let Some(group) = self.settings.coverage.group(key) else {
            return;
        };
        if checked == group.entries.contains(entry) {
            return;
        }
        self.settings.coverage.separate_groups();
        if checked {
            self.settings.coverage.add(key, entry);
        } else {
            self.settings.coverage.take_out(key, entry);
        }
        self.save();
        self.invalidate_coverage_view();
    }

    pub(crate) fn coverage_toggled(&mut self, id: &str, checked: bool) {
        let choice = if checked {
            Keep::Original
        } else {
            Keep::Translate
        };
        let state = if checked { State::All } else { State::None };
        if let Some(index) = row_index(id, "groups/").and_then(|index| index.checked_sub(1)) {
            let Some(group) = self.settings.coverage.groups().into_iter().nth(index) else {
                return;
            };
            let targets = group
                .entries
                .iter()
                .map(|entry| entry.split(':').next().unwrap_or(entry).to_owned())
                .collect::<Vec<_>>();
            if let Some(view) = &mut self.coverage.browser {
                view.group_marks.insert(group.key, state);
                for row in &mut view.rows {
                    if !row.folder
                        && targets
                            .iter()
                            .any(|target| crate::coverage::entry_matches(target, &row.path))
                    {
                        row.state = state;
                    }
                }
            }
            self.apply_marks(&targets, None, choice);
            return;
        }
        if let Some(index) = row_index(id, "sheet-mark-")
            && let Some(row) = self
                .coverage
                .browser
                .as_mut()
                .and_then(|view| view.rows.get_mut(index))
        {
            row.state = state;
            let entry = if row.folder {
                format!("{}/*", row.path)
            } else {
                row.path.clone()
            };
            let part = row.part;
            self.apply_marks(&[entry], part, choice);
        }
    }

    fn browser_click(&mut self, id: &str) -> bool {
        let Some(view) = &self.coverage.browser else {
            return false;
        };
        if id == "mark-shown" {
            if self.coverage.browser_revision != Some(self.coverage.revision) {
                return true;
            }
            let targets = view.targets.clone();
            let choice = if view.all_marked {
                Keep::Translate
            } else {
                Keep::Original
            };
            self.apply_marks(&targets, self.coverage.filter.part, choice);
            return true;
        }
        let Some((index, command)) = ["sheet-row-", "sheet-details-"]
            .into_iter()
            .find_map(|prefix| row_index(id, prefix).map(|index| (index, prefix)))
        else {
            return false;
        };
        let Some(row) = view.rows.get(index) else {
            return false;
        };
        let path = row.path.clone();
        match command {
            "sheet-details-" => self.coverage.details = Some(row.clone()),
            "sheet-row-" if row.expansion != Expansion::Leaf => {
                if row.expansion.can_toggle() {
                    if !self.coverage.filter.open.remove(&path) {
                        self.coverage.filter.open.insert(path);
                    }
                    self.invalidate_coverage_view();
                }
            }
            _ => return false,
        }
        true
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn coverage_click(&mut self, id: &str) -> bool {
        if self.browser_click(id) {
            return true;
        }
        match id {
            "groups" | "new-group" => {
                self.new_coverage_group(None);
                return true;
            }
            "group-with-sheet" => {
                let entry = self.coverage.membership.take();
                self.new_coverage_group(entry);
                return true;
            }
            "membership-done" => {
                self.coverage.membership = None;
                return true;
            }
            "group-actions" => {
                self.coverage.group_actions = !self.coverage.group_actions;
                return true;
            }
            "coverage-filters" => {
                self.coverage.filters_open = !self.coverage.filters_open;
                return true;
            }
            "details-done" => {
                self.coverage.details = None;
                return true;
            }
            "details-groups" => {
                if let Some(row) = self.coverage.details.take() {
                    self.coverage.membership = Some(if row.folder {
                        format!("{}/*", row.path)
                    } else {
                        row.path
                    });
                }
                return true;
            }
            "details-link" => {
                if let Some(row) = &self.coverage.details {
                    let mut encoded = String::new();
                    for byte in row.path.bytes() {
                        if byte.is_ascii_alphanumeric() || b"-_./".contains(&byte) {
                            encoded.push(char::from(byte));
                        } else {
                            let _ = write!(encoded, "%{byte:02X}");
                        }
                    }
                    if let Err(error) =
                        angelica_sdk::links::open(&format!("https://xiviewer.app/sheet/{encoded}"))
                    {
                        angelica_sdk::warning(&error.message);
                    }
                }
                return true;
            }
            "coverage-clear" => self.coverage.filter.query.clear(),
            "coverage-collapse" => self.coverage.filter.open.clear(),
            _ => {
                let Some(group) = self.coverage_group() else {
                    return false;
                };
                let replacement = row_index(id, "add-")
                    .and_then(|index| {
                        self.coverage
                            .browser
                            .as_ref()?
                            .candidates
                            .get(index)
                            .cloned()
                    })
                    .or_else(|| {
                        (id == "add-typed").then(|| {
                            self.coverage
                                .adding
                                .as_deref()
                                .unwrap_or("")
                                .trim()
                                .to_owned()
                        })
                    })
                    .filter(|text| !text.is_empty());
                let remove = row_index(id, "remove-entry-")
                    .and_then(|index| group.entries.get(index))
                    .or_else(|| {
                        row_index(id, "remove-missing-")
                            .and_then(|index| self.coverage.browser.as_ref()?.missing.get(index))
                    });
                if let Some(entry) = replacement {
                    self.settings.coverage.separate_groups();
                    self.settings.coverage.add(&group.key, &entry);
                    if id == "add-typed" {
                        self.coverage.adding = Some(String::new());
                    }
                    self.save();
                } else if let Some(entry) = remove.cloned() {
                    self.settings.coverage.separate_groups();
                    self.settings.coverage.take_out(&group.key, &entry);
                    self.save();
                } else {
                    match id {
                        "add-sheets" => {
                            self.coverage.adding = Some(String::new());
                            self.coverage.renaming = None;
                        }
                        "adding-done" => self.coverage.adding = None,
                        "rename" => {
                            self.coverage.renaming = Some(group.title(self.russian));
                            self.coverage.adding = None;
                        }
                        "rename-cancel" => self.coverage.renaming = None,
                        "rename-save" => {
                            if let Some(name) = self.coverage.renaming.take() {
                                self.settings.coverage.separate_groups();
                                self.settings.coverage.rename(&group.key, &name);
                                self.save();
                            }
                        }
                        "delete" => self.coverage.deleting = Some(group.key),
                        "delete-no" => self.coverage.deleting = None,
                        "delete-yes" => {
                            if let Some(key) = self.coverage.deleting.take() {
                                self.settings.coverage.separate_groups();
                                self.settings.coverage.remove(&key);
                                self.coverage.group = None;
                                self.save();
                            }
                        }
                        "restore" => {
                            self.settings.coverage.separate_groups();
                            self.settings.coverage.restore(&group.key);
                            self.save();
                        }
                        _ => return false,
                    }
                }
            }
        }
        self.refresh_tree();
        true
    }
}
