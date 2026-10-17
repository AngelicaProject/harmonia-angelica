//! The Dictionary tab: a name in any language, found in the game's
//! language and the translation. A click on a name chooses it; the chosen
//! name shows the other languages and what can be done with it.
use crate::{Harmonia, Side, count, names, tab_settings::endonym};
use angelica_sdk::{
    clipboard,
    ui::{ActionKind, Entry, Role, Tone, Window},
};
use names::{Category, Name};

/// A category's name for one of its names.
pub(crate) fn category_word(category: Category, russian: bool) -> &'static str {
    match (category, russian) {
        (Category::Item, false) => "item",
        (Category::Item, true) => "предмет",
        (Category::Action, false) => "action",
        (Category::Action, true) => "действие",
        (Category::Status, false) => "status",
        (Category::Status, true) => "статус",
        (Category::Place, false) => "place",
        (Category::Place, true) => "место",
        (Category::Duty, false) => "duty",
        (Category::Duty, true) => "сражение",
        (Category::Quest, false) => "quest",
        (Category::Quest, true) => "задание",
    }
}

fn category_plural(category: Category, russian: bool) -> &'static str {
    match (category, russian) {
        (Category::Item, false) => "Items",
        (Category::Item, true) => "Предметы",
        (Category::Action, false) => "Actions",
        (Category::Action, true) => "Действия",
        (Category::Status, false) => "Statuses",
        (Category::Status, true) => "Статусы",
        (Category::Place, false) => "Places",
        (Category::Place, true) => "Места",
        (Category::Duty, false) => "Duties",
        (Category::Duty, true) => "Сражения",
        (Category::Quest, false) => "Quests",
        (Category::Quest, true) => "Задания",
    }
}

fn number(category: Category) -> usize {
    Category::ALL
        .iter()
        .position(|c| *c == category)
        .unwrap_or(0)
}

/// A name's page on the Console Games Wiki, by its English name.
fn wiki_page(english: &str) -> String {
    use std::fmt::Write;
    let mut url = String::from("https://ffxiv.consolegameswiki.com/wiki/");
    for byte in english.trim().bytes() {
        match byte {
            b' ' => url.push('_'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                url.push(char::from(byte));
            }
            _ => {
                let _ = write!(url, "%{byte:02X}");
            }
        }
    }
    url
}

/// A name's page on Garland Tools, for the categories it numbers as the
/// game does.
fn garland_page(category: Category, row: u32) -> Option<String> {
    let kind = match category {
        Category::Item => "item",
        Category::Action => "action",
        Category::Status => "status",
        Category::Quest => "quest",
        Category::Place | Category::Duty => return None,
    };
    Some(format!("https://www.garlandtools.org/db/#{kind}/{row}"))
}

/// A name's category and row from the end of a part's id.
fn key(rest: &str) -> Option<(Category, u32)> {
    let (category, row) = rest.split_once('-')?;
    Some((
        *Category::ALL.get(category.parse::<usize>().ok()?)?,
        row.parse().ok()?,
    ))
}

impl Harmonia {
    fn found_name(&self, key: (Category, u32)) -> Option<&Name> {
        self.dictionary
            .found
            .as_ref()?
            .matches
            .iter()
            .find(|name| (name.category, name.row) == key)
    }

    fn name_entry(&self, name: &Name) -> Entry {
        let key = (name.category, name.row);
        let id = format!("{}-{}", number(name.category), name.row);
        let chosen = self.dictionary.chosen == Some(key);
        let mut entry = Entry::new(format!("name-{id}"), &name.shown).chosen(chosen);
        if name.icon > 0 {
            entry = entry.icon(name.icon);
        }
        if name.is_translated() {
            entry = entry.note(name.original());
        }
        entry = entry.value(if self.dictionary.copied == Some(key) {
            self.tr("copied", "скопировано")
        } else {
            category_word(name.category, self.russian)
        });
        if !chosen {
            return entry;
        }
        for original in name.originals.iter().skip(1) {
            entry = entry.fact(endonym(&original.language), &original.text);
        }
        entry = entry.more(
            format!("copy-o-{id}"),
            if self.russian {
                format!("Копировать «{}»", name.original())
            } else {
                format!("Copy “{}”", name.original())
            },
            ActionKind::Normal,
            true,
        );
        if name.is_translated() {
            entry = entry.more(
                format!("copy-s-{id}"),
                if self.russian {
                    format!("Копировать «{}»", name.shown)
                } else {
                    format!("Copy “{}”", name.shown)
                },
                ActionKind::Quiet,
                true,
            );
        }
        if name.wearable {
            entry = entry.more(
                format!("try-{id}"),
                self.tr("Try on", "Примерить"),
                ActionKind::Quiet,
                true,
            );
        }
        if !name.english.is_empty() {
            entry = entry.more(
                format!("wiki-c-{id}"),
                self.tr("Open in Console Games Wiki", "Открыть в Console Games Wiki"),
                ActionKind::Quiet,
                true,
            );
        }
        if garland_page(name.category, name.row).is_some() {
            entry = entry.more(
                format!("wiki-g-{id}"),
                self.tr("Open in Garland Tools", "Открыть в Garland Tools"),
                ActionKind::Quiet,
                true,
            );
        }
        entry
    }

    fn filters(&self, window: Window) -> Window {
        let by_category = self
            .dictionary
            .found
            .as_ref()
            .map(|found| found.by_category);
        window.row(|mut row| {
            let all = self.tr("All", "Все");
            row = if self.dictionary.filter.is_none() {
                row.styled(all, Role::Value, Tone::Normal)
            } else {
                row.action("filter-all", all, ActionKind::Quiet, true)
            };
            for category in Category::ALL {
                let mut label = category_plural(category, self.russian).to_owned();
                if let Some(counts) = by_category
                    && counts[number(category)] > 0
                {
                    label = format!("{label} · {}", counts[number(category)]);
                }
                row = if self.dictionary.filter == Some(category) {
                    row.styled(label, Role::Value, Tone::Normal)
                } else {
                    row.action(
                        format!("filter-{}", number(category)),
                        label,
                        ActionKind::Quiet,
                        true,
                    )
                };
            }
            row
        })
    }

    pub(crate) fn dictionary_tab(&self, window: Window) -> Window {
        let mut window = window.input(
            "query",
            "",
            &self.dictionary.query,
            self.tr("A name in any language", "Название на любом языке"),
        );
        window = self.filters(window);
        if let Some(fraction) = self.side_running(&Side::Names(String::new())) {
            window = window.progress(
                fraction,
                self.tr("The dictionary is being prepared", "Словарь готовится"),
            );
        }
        if let Some(reason) = &self.dictionary.failed {
            window = window.toned(
                if self.russian {
                    format!("Словарь не собрался: {reason}")
                } else {
                    format!("The dictionary could not be built: {reason}")
                },
                Tone::Warning,
            );
        }
        if let Some(built) = &self.dictionary.built
            && !built.missing.is_empty()
        {
            let missing: Vec<&str> = built
                .missing
                .iter()
                .map(|c| category_plural(*c, self.russian))
                .collect();
            window = window.muted(if self.russian {
                format!(
                    "Пока без этих видов: {}. Карта игры ещё не называет столбцы их листов.",
                    missing.join(", ").to_lowercase()
                )
            } else {
                format!(
                    "Not yet: {}. The game map does not name the columns of their sheets yet.",
                    missing.join(", ").to_lowercase()
                )
            });
        }
        let query = self.dictionary.query.trim();
        if query.chars().count() < names::MIN_QUERY {
            return window
                .muted(self.tr("In chat: /hfind and a name.", "В чате: /hfind и название."));
        }
        let Some(found) = &self.dictionary.found else {
            return window;
        };
        if found.matches.is_empty() {
            window = window.muted(self.tr("Nothing found.", "Ничего не найдено."));
        }
        for category in Category::ALL {
            let names: Vec<&Name> = found
                .matches
                .iter()
                .filter(|name| name.category == category)
                .collect();
            if names.is_empty() {
                continue;
            }
            window = window.heading(format!(
                "{} · {}",
                category_plural(category, self.russian),
                count(found.by_category[number(category)] as u64, self.russian)
            ));
            for name in names {
                let entry = self.name_entry(name);
                window = window.entry(entry);
            }
        }
        if found.total > found.matches.len() {
            window = window.muted(if self.russian {
                format!(
                    "Показаны первые {} из {}. Допишите название, чтобы сузить поиск.",
                    found.matches.len(),
                    count(found.total as u64, true)
                )
            } else {
                format!(
                    "Showing the first {} of {}. Type more of the name to narrow it down.",
                    found.matches.len(),
                    count(found.total as u64, false)
                )
            });
        }
        window.muted(format!("{} /hfind {query}", self.tr("In chat:", "В чате:")))
    }

    fn copy(text: &str) {
        if let Err(error) = clipboard::copy(text) {
            angelica_sdk::warning(&format!("Cannot copy: {}", error.message));
        }
    }

    /// Handles a click of the Dictionary tab; false for a part it does not
    /// have.
    pub(crate) fn dictionary_click(&mut self, id: &str) -> bool {
        if id == "filter-all" {
            self.dictionary.filter = None;
        } else if let Some(category) = id
            .strip_prefix("filter-")
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| Category::ALL.get(n))
        {
            self.dictionary.filter = Some(*category);
        } else if let Some(key) = id.strip_prefix("name-").and_then(key) {
            self.dictionary.chosen = Some(key);
            return true;
        } else if let Some(rest) = id
            .strip_prefix("copy-o-")
            .or_else(|| id.strip_prefix("copy-s-"))
        {
            let shown = id.starts_with("copy-s-");
            if let Some(key) = key(rest)
                && let Some(name) = self.found_name(key)
            {
                let text = if shown {
                    name.shown.clone()
                } else {
                    name.original().to_owned()
                };
                Self::copy(&text);
                self.dictionary.copied = Some(key);
            }
            return true;
        } else if let Some(key) = id.strip_prefix("try-").and_then(key) {
            if let Err(error) = angelica_sdk::lens::act("try_on", &[("item", key.1.into())]) {
                angelica_sdk::warning(&format!("Cannot try on: {}", error.message));
            }
            return true;
        } else if let Some(rest) = id
            .strip_prefix("wiki-c-")
            .or_else(|| id.strip_prefix("wiki-g-"))
        {
            let url = key(rest).and_then(|key| {
                if id.starts_with("wiki-g-") {
                    garland_page(key.0, key.1)
                } else {
                    self.found_name(key)
                        .filter(|name| !name.english.is_empty())
                        .map(|name| wiki_page(&name.english))
                }
            });
            if let Some(url) = url
                && let Err(error) = angelica_sdk::links::open(&url)
            {
                angelica_sdk::warning(&format!("Cannot open {url}: {}", error.message));
            }
            return true;
        } else {
            return false;
        }
        self.dictionary.again = true;
        true
    }
}
