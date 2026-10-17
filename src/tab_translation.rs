//! The Translation tab: what the game shows now in large text, the
//! translations with their facts and actions, the release notes, and on
//! the first run the suggested translation with a field for a link.
use crate::{
    FeedStatus, Harmonia, Inspection, Job, SUGGESTED_FEED, count, language_name, library,
    megabytes, short, words,
};
use angelica_sdk::ui::{ActionKind, Dialog, Entry, Mark, Meter, Readout, Role, Tone, Window};
use library::Translation;

impl Harmonia {
    pub(crate) fn translation_tab(&self, mut window: Window) -> Window {
        let first_run = self.library.is_empty() && self.settings.feeds.is_empty();
        if first_run {
            window = self.first_run(window);
        } else {
            window = self.hero(window);
        }
        if let Some(restart) = self.restart_line() {
            window = window.toned(restart, Tone::Warning);
        }
        if !self.started_with_game && !first_run {
            window = window.toned(
                self.tr(
                    "Harmonia was enabled after the game started; translations apply when the game starts.",
                    "Harmonia включена после запуска игры; переводы применяются при запуске игры.",
                ),
                Tone::Warning,
            );
        }
        if !first_run {
            window = self.rows(window.heading(self.tr("Translations", "Переводы")));
        }
        window = self.imports(window);
        if matches!(&self.job, Some((_, job)) if !matches!(job, Job::Download | Job::Compile(_))) {
            window = window.progress(self.progress.0, &self.progress.1);
        }
        if !self.message.0.is_empty() {
            window = window.toned(&self.message.0, self.message.1);
        }
        if !first_run {
            window = self.link(window);
            window = self.news(window);
        }
        window
    }

    /// The state of the game in large text: the language, the rows
    /// translated so far, and how well the translation matches this game
    /// build.
    fn hero(&self, mut window: Window) -> Window {
        if self.yielded {
            return window
                .styled(
                    self.tr("Harmonia in Dalamud", "Harmonia в Dalamud"),
                    Role::Big,
                    Tone::Normal,
                )
                .muted(self.tr(
                    "Harmonia in Dalamud translated the game in the previous session, so this copy stays off in this one. It translates from the next start once Harmonia in Dalamud does not run, for example while Dalamud waits for an update after a game update.",
                    "В прошлой игре перевод делала Harmonia в Dalamud, поэтому в этой игре эта копия выключена. Она начнёт переводить со следующего запуска, если Harmonia в Dalamud не запустится, например пока Dalamud ждёт обновления после обновления игры.",
                ));
        }
        let Some(applied) = self.applied.as_deref().and_then(|h| self.translation(h)) else {
            let game = language_name(crate::names::tag(self.game_language()), self.russian);
            return window
                .styled(
                    self.tr("No translation", "Без перевода"),
                    Role::Big,
                    Tone::Normal,
                )
                .muted(if self.russian {
                    format!("В игре её язык: {game}")
                } else {
                    format!("The game shows its own language: {game}")
                });
        };
        window = window
            .styled(
                language_name(&applied.language, self.russian),
                Role::Big,
                Tone::Normal,
            )
            .muted(if self.russian {
                format!("{} · версия {}", applied.title, applied.version)
            } else {
                format!("{} · version {}", applied.title, applied.version)
            });
        if let Some(status) = &self.status {
            #[allow(clippy::cast_precision_loss)]
            let taken = status.taken as f64;
            window = window.row(|row| {
                row.readout(Readout::new("taken", taken).role(Role::Title))
                    .muted(self.tr("rows in the game", "строк в игре"))
            });
            let seen = status.taken + status.stale;
            if seen > 0 {
                #[allow(clippy::cast_precision_loss)]
                let share = status.taken as f64 / seen as f64;
                let percent = (share * 100.0).floor();
                window = window.meter(Meter::new(share, 1.0).tone(Tone::Good).label(
                    if self.russian {
                        format!("{percent}% совпадает с этой версией игры")
                    } else {
                        format!("{percent}% matches this game build")
                    },
                ));
            }
        }
        if let Some(summary) = &self.summary
            && summary.kept_cells > 0
        {
            window = window.muted(if self.russian {
                format!(
                    "На языке игры: листов {}, ячеек {}",
                    count(summary.kept_sheets as u64, true),
                    count(summary.kept_cells as u64, true)
                )
            } else {
                format!(
                    "In the game's language: {} sheets, {} cells",
                    count(summary.kept_sheets as u64, false),
                    count(summary.kept_cells as u64, false)
                )
            });
        }
        window
    }

    /// No translation installed yet: the suggested one, then a field for a
    /// link from a translation's author.
    fn first_run(&self, window: Window) -> Window {
        let mut suggestion = Entry::new("suggest", language_name("ru", self.russian))
            .note("Project Prima")
            .plain()
            .chosen(true);
        let release = self.suggestion.as_ref().and_then(|c| c.release.as_ref());
        if let Some(release) = release {
            suggestion = suggestion
                .value(megabytes(release.size))
                .fact(self.tr("Version", "Версия"), &release.version)
                .fact(
                    self.tr("Made for the game", "Для игры"),
                    &release.game_version,
                );
        }
        let busy = self.downloading.as_deref() == Some(SUGGESTED_FEED)
            || self.wanted.iter().any(|w| w == SUGGESTED_FEED);
        if busy {
            suggestion = suggestion.progress(self.progress.0);
        }
        window
            .styled(
                self.tr("No translation yet", "Перевода пока нет"),
                Role::Big,
                Tone::Normal,
            )
            .heading(self.tr("Suggested", "Предложение"))
            .entry(suggestion)
            .row(|row| {
                row.action(
                    "suggest-install",
                    self.tr("Install", "Установить"),
                    ActionKind::Main,
                    !busy && self.job.is_none(),
                )
            })
            .heading(self.tr("Your own link", "Своя ссылка"))
            .input(
                "feed",
                "",
                &self.feed_input,
                self.tr(
                    "A link from the translation's author",
                    "Ссылка от автора перевода",
                ),
            )
            .row(|row| {
                row.action(
                    "feed-add",
                    self.tr("Add", "Добавить"),
                    ActionKind::Normal,
                    true,
                )
            })
            .muted(self.tr(
                "A .hpk file goes into the import folder of Harmonia's folder: ··· → Open folder.",
                "Файл .hpk положите в папку import в папке Harmonia: ··· → Открыть папку.",
            ))
    }

    /// The release notes of the translation the player opened them for.
    fn news(&self, window: Window) -> Window {
        let Some(hash) = &self.news else {
            return window;
        };
        let Some(translation) = self.translation(hash) else {
            return window;
        };
        let available = translation.feed.as_deref().and_then(|url| {
            self.feeds
                .iter()
                .find(|(feed, _)| feed == url)
                .and_then(|(_, view)| match &view.status {
                    FeedStatus::Available(release) => Some(release),
                    _ => None,
                })
        });
        let mut window = window;
        if let Some(release) = available
            && let Some(changelog) = &release.changelog
        {
            window = window
                .heading(if self.russian {
                    format!("Что нового в {}", release.version)
                } else {
                    format!("What's new in {}", release.version)
                })
                .text(changelog);
        }
        if let Some(changelog) = &translation.changelog {
            window = window
                .heading(if self.russian {
                    format!("Что нового в {}", translation.version)
                } else {
                    format!("What's new in {}", translation.version)
                })
                .text(changelog);
        }
        window
    }

    /// When the feeds were checked last, in words.
    fn checked_ago(&self) -> Option<String> {
        let at = self.checked_at?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let minutes = ((self.clock - at) / 60.0).floor() as u64;
        Some(match (minutes, self.russian) {
            (0, false) => "checked just now".to_owned(),
            (0, true) => "проверено только что".to_owned(),
            (m, false) => format!("checked {m} min ago"),
            (m, true) => format!("проверено {m} мин назад"),
        })
    }

    /// What the applied translation did in this session.
    fn session_facts(&self, mut entry: Entry) -> Entry {
        if let Some(status) = &self.status {
            entry = entry.fact(
                self.tr("Rows in the game", "Строк в игре"),
                count(status.taken, self.russian),
            );
            entry = entry.fact(
                self.tr(
                    "Left as is: the game's text changed",
                    "Пропущено: текст игры изменился",
                ),
                count(status.stale, self.russian),
            );
            if status.refused > 0 {
                entry = entry.fact(
                    self.tr("Left untouched", "Не тронуто"),
                    if self.russian {
                        format!("строк с неожиданным устройством: {}", status.refused)
                    } else {
                        format!("{} rows with an unexpected layout", status.refused)
                    },
                );
            }
            if !status.mismatched.is_empty() {
                // Each is the sheet's name, a colon, and why.
                let names: Vec<_> = status
                    .mismatched
                    .iter()
                    .take(8)
                    .map(|line| line.split(':').next().unwrap_or(line))
                    .collect();
                entry = entry.fact(
                    self.tr(
                        "Changed in this game build, in its language",
                        "Изменились в этой версии игры, на её языке",
                    ),
                    format!("{} ({})", status.mismatched.len(), names.join(", ")),
                );
            }
        }
        if self.fonts > 0 {
            entry = entry.fact(
                self.tr("Fonts", "Шрифты"),
                if self.russian {
                    format!("файлов шрифтов игры с буквами перевода: {}", self.fonts)
                } else {
                    format!(
                        "{} game font files with the translation's glyphs",
                        self.fonts
                    )
                },
            );
        }
        entry
    }

    /// An installed translation as a row: its language and title, where it
    /// stands, and under the pointer Use; the chosen one shows what it is.
    #[allow(clippy::too_many_lines)]
    fn translation_entry(&self, index: usize, translation: &Translation) -> Entry {
        let id = format!("row-t{index}");
        let hash = translation.hash.as_str();
        let applied = self.applied.as_deref() == Some(hash);
        let active = self.settings.active.as_deref() == Some(hash);
        let idle = self.job.is_none();
        let (state, mark) = match (applied, active) {
            (true, true) => (self.tr("in the game", "в игре"), Mark::On),
            (false, true) => (
                self.tr("after a restart", "после перезапуска"),
                Mark::Attention,
            ),
            (true, false) => (self.tr("until you quit", "до выхода из игры"), Mark::Off),
            (false, false) => (self.tr("installed", "установлен"), Mark::None),
        };
        let chosen = self.chosen.as_deref() == Some(id.as_str());
        let mut note = format!("{} · {}", translation.title, translation.version);
        if translation.channel == "testing" {
            note.push_str(self.tr(" · testing", " · тестовая"));
        }
        let available = translation.feed.as_deref().and_then(|url| {
            let (_, view) = self.feeds.iter().find(|(feed, _)| feed == url)?;
            match &view.status {
                FeedStatus::Available(release) => Some(release.version.clone()),
                _ => None,
            }
        });
        let mut entry = Entry::new(&id, language_name(&translation.language, self.russian))
            .plain()
            .note(note)
            .value(if available.is_some() {
                self.tr("new version", "новая версия")
            } else {
                state
            })
            .mark(if available.is_some() {
                Mark::Attention
            } else {
                mark
            })
            .chosen(chosen);
        let busy = matches!(&self.job, Some((_, Job::Compile(compiling))) if compiling == hash)
            || (translation.feed.is_some() && self.downloading == translation.feed);
        if busy {
            entry = entry
                .progress(self.progress.0)
                .value(if self.progress.1.is_empty() {
                    self.tr("preparing", "подготовка").to_owned()
                } else {
                    self.progress.1.clone()
                });
        }
        if available.is_some()
            && idle
            && let Some(position) = self.feed_position(translation)
        {
            entry = entry.action(
                format!("feed-get-{position}"),
                self.tr("Update", "Обновить"),
            );
        }
        if !active && idle {
            entry = entry.action(format!("use-{index}"), self.tr("Use", "Использовать"));
        }
        if !chosen {
            return entry;
        }
        entry = entry
            .fact(
                self.tr("Version", "Версия"),
                if self.russian {
                    format!(
                        "{}, для игры {}",
                        translation.version, translation.game_version
                    )
                } else {
                    format!(
                        "{}, for the game {}",
                        translation.version, translation.game_version
                    )
                },
            )
            .fact(self.tr("Team", "Команда"), &translation.team)
            .fact(
                self.tr("Translated", "Переведено"),
                if self.russian {
                    format!("ячеек: {}", count(translation.cells as u64, true))
                } else {
                    format!("{} cells", count(translation.cells as u64, false))
                },
            )
            .fact(
                self.tr("Author's key", "Ключ автора"),
                translation
                    .key
                    .as_deref()
                    .map_or_else(|| self.tr("not signed", "без подписи").to_owned(), short),
            )
            .fact(
                self.tr("Updates", "Обновления"),
                translation.feed.as_deref().map_or_else(
                    || self.tr("from a file", "из файла").to_owned(),
                    |url| {
                        let how = if self.settings.auto_install {
                            self.tr("install by themselves", "ставятся сами")
                        } else {
                            self.tr("on your word", "по вашему слову")
                        };
                        let mut text = format!("{how}; {}", self.feed_state(url).0);
                        if let Some(ago) = self.checked_ago() {
                            text = format!("{text} ({ago})");
                        }
                        text
                    },
                ),
            );
        if applied {
            entry = self.session_facts(entry);
        }
        self.translation_actions(entry, index, translation)
    }

    fn feed_position(&self, translation: &Translation) -> Option<usize> {
        translation
            .feed
            .as_deref()
            .and_then(|url| self.settings.feeds.iter().position(|f| f == url))
    }

    /// What can be done with the chosen translation.
    fn translation_actions(
        &self,
        mut entry: Entry,
        index: usize,
        translation: &Translation,
    ) -> Entry {
        let idle = self.job.is_none();
        let active = self.settings.active.as_deref() == Some(translation.hash.as_str());
        let has_news = translation.changelog.is_some()
            || translation.feed.as_deref().is_some_and(|url| {
                self.feeds.iter().any(|(feed, view)| {
                    feed == url
                        && matches!(&view.status, FeedStatus::Available(release) if release.changelog.is_some())
                })
            });
        if has_news {
            let open = self.news.as_deref() == Some(translation.hash.as_str());
            entry = entry.more(
                format!("news-{index}"),
                if open {
                    self.tr("Hide what's new", "Скрыть, что нового")
                } else {
                    self.tr("What's new", "Что нового")
                },
                ActionKind::Normal,
                true,
            );
        }
        if active {
            entry = entry.more(
                "none",
                self.tr("Stop using", "Не использовать"),
                ActionKind::Normal,
                idle,
            );
        }
        if let Some(position) = self.feed_position(translation) {
            entry = entry.more(
                format!("feed-remove-{position}"),
                self.tr("Turn off updates", "Отключить обновления"),
                ActionKind::Quiet,
                idle,
            );
        }
        entry.more(
            format!("remove-{index}"),
            self.tr("Remove", "Удалить"),
            ActionKind::Danger,
            idle,
        )
    }

    /// A feed nothing is installed from yet: its size and Download, or how
    /// far its download is.
    fn feed_entry(&self, index: usize, url: &str) -> Entry {
        let id = format!("row-f{index}");
        let view = self
            .feeds
            .iter()
            .find(|(feed, _)| feed == url)
            .map(|(_, v)| v);
        let title = view.and_then(|view| view.title.as_deref()).unwrap_or(url);
        let language = view
            .and_then(|view| view.newest.as_ref())
            .map(|r| r.language.clone());
        let chosen = self.chosen.as_deref() == Some(id.as_str());
        let mut entry = match &language {
            Some(language) => Entry::new(&id, language_name(language, self.russian)).note(title),
            None => Entry::new(&id, title),
        }
        .plain()
        .chosen(chosen);
        if self.downloading.as_deref() == Some(url) {
            entry = entry.progress(self.progress.0).value(&self.progress.1);
        } else {
            match view.map(|view| &view.status) {
                Some(FeedStatus::Available(release)) => {
                    entry = entry.value(megabytes(release.size));
                    let waits = self.pending.iter().any(|pending| pending.feed == url);
                    if !waits && self.job.is_none() {
                        entry = entry
                            .action(format!("feed-get-{index}"), self.tr("Download", "Скачать"));
                    }
                    if chosen {
                        entry = entry.fact(self.tr("Version", "Версия"), &release.version);
                    }
                }
                Some(FeedStatus::Problem(reason)) => {
                    entry = entry.value(reason).mark(Mark::Attention);
                }
                _ => entry = entry.value(self.tr("checking", "проверяется")),
            }
        }
        if chosen {
            entry = entry.fact(self.tr("Link", "Ссылка"), url).more(
                format!("feed-remove-{index}"),
                self.tr("Forget the link", "Забыть ссылку"),
                ActionKind::Quiet,
                self.job.is_none(),
            );
        }
        entry
    }

    /// Every translation, then the game without one.
    fn rows(&self, mut window: Window) -> Window {
        for (index, translation) in self.library.iter().enumerate() {
            window = window.entry(self.translation_entry(index, translation));
        }
        for (index, url) in self.settings.feeds.iter().enumerate() {
            if !self
                .library
                .iter()
                .any(|t| t.feed.as_deref() == Some(url.as_str()))
            {
                window = window.entry(self.feed_entry(index, url));
            }
        }
        let original = language_name(crate::names::tag(self.game_language()), self.russian);
        let applied = self.applied.is_none();
        let active = self.settings.active.is_none();
        let (state, mark) = match (applied, active) {
            (true, true) => (self.tr("in the game", "в игре"), Mark::On),
            (false, true) => (
                self.tr("after a restart", "после перезапуска"),
                Mark::Attention,
            ),
            _ => ("", Mark::None),
        };
        let mut none = Entry::new("row-none", self.tr("No translation", "Без перевода"))
            .plain()
            .note(if self.russian {
                format!("язык игры: {original}")
            } else {
                format!("the game's language: {original}")
            })
            .value(state)
            .mark(mark);
        if !active && self.job.is_none() {
            none = none.action("use-none", self.tr("Use", "Использовать"));
        }
        window.entry(none)
    }

    /// Files in the import folder.
    fn imports(&self, mut window: Window) -> Window {
        if !self.imports.is_empty() {
            window = window.heading(self.tr("Files", "Файлы"));
        }
        for (index, (path, size)) in self.imports.iter().enumerate() {
            let name = path.rsplit('/').next().unwrap_or(path);
            let mut entry = Entry::new(format!("row-i{index}"), name)
                .plain()
                .value(megabytes(*size));
            if self.job.is_none() {
                entry = entry.action(format!("check-{index}"), self.tr("Check", "Проверить"));
            }
            window = window.entry(entry);
        }
        window
    }

    /// The field for a link, or the word that opens it, and the check for
    /// updates.
    fn link(&self, window: Window) -> Window {
        let checking = matches!(&self.job, Some((_, Job::Feeds)));
        if !self.adding {
            return window.row(|row| {
                let row = row
                    .action(
                        "feed-open",
                        self.tr("+ Add by link", "+ Добавить по ссылке"),
                        ActionKind::Normal,
                        true,
                    )
                    .spacer();
                let row = match self.checked_ago() {
                    Some(ago) if !self.settings.feeds.is_empty() => row.muted(ago),
                    _ => row,
                };
                if self.settings.feeds.is_empty() {
                    row
                } else {
                    row.action(
                        "feed-check",
                        if checking {
                            self.tr("Checking…", "Проверка…")
                        } else {
                            self.tr("Check for updates", "Проверить обновления")
                        },
                        ActionKind::Quiet,
                        !checking && self.job.is_none(),
                    )
                }
            });
        }
        window
            .input(
                "feed",
                "",
                &self.feed_input,
                self.tr(
                    "A link from the translation's author",
                    "Ссылка от автора перевода",
                ),
            )
            .row(|row| {
                row.action(
                    "feed-add",
                    self.tr("Add", "Добавить"),
                    ActionKind::Main,
                    true,
                )
                .action(
                    "feed-close",
                    self.tr("Cancel", "Отмена"),
                    ActionKind::Quiet,
                    true,
                )
            })
    }

    /// The question whether to trust the publisher of a link just added.
    fn trust_dialog(&self, url: &str) -> Option<Dialog> {
        let (_, view) = self.feeds.iter().find(|(feed, _)| feed == url)?;
        let FeedStatus::Available(release) = &view.status else {
            return None;
        };
        let title = view.title.as_deref().unwrap_or(url);
        let dialog = Dialog::new(
            "trust",
            self.tr("Trust this publisher?", "Доверять издателю?"),
        )
        .styled(title, Role::Title, Tone::Normal)
        .fact(
            self.tr("Language", "Язык"),
            language_name(&release.language, self.russian),
        )
        .fact(self.tr("Version", "Версия"), &release.version)
        .fact(self.tr("Size", "Размер"), megabytes(release.size));
        let dialog = match &view.key {
            Some(key) => dialog
                .fact(
                    self.tr("Publisher", "Издатель"),
                    format!("{} {}", self.tr("key", "ключ"), short(key)),
                )
                .text(self.tr(
                    "The publisher signs translations with this key.",
                    "Издатель подписывает переводы этим ключом.",
                )),
            None => dialog.toned(
                self.tr(
                    "The feed names no publisher key: whoever made the pack cannot be checked.",
                    "Лента не называет ключ издателя: кто сделал пак, проверить нельзя.",
                ),
                Tone::Warning,
            ),
        };
        Some(dialog.row(|row| {
            row.action(
                "trust",
                self.tr("Trust and download", "Доверять и скачать"),
                ActionKind::Main,
                true,
            )
            .action(
                "distrust",
                self.tr("Cancel", "Отмена"),
                ActionKind::Quiet,
                true,
            )
        }))
    }

    /// The question whether to install the pack that was checked.
    fn inspection_dialog(&self, inspection: &Inspection) -> Dialog {
        let translation = &inspection.translation;
        let dialog = Dialog::new(
            "install",
            self.tr("Install this translation?", "Установить перевод?"),
        )
        .styled(
            format!("{} {}", translation.title, translation.version),
            Role::Title,
            Tone::Normal,
        )
        .fact(self.tr("Team", "Команда"), &translation.team)
        .fact(
            self.tr("Language", "Язык"),
            format!("{} ← {}", translation.language, translation.game_language),
        )
        .fact(
            self.tr("Translated", "Переведено"),
            if self.russian {
                format!(
                    "ячеек: {}, листов: {}",
                    translation.cells, translation.sheets
                )
            } else {
                format!(
                    "{} cells in {} sheets",
                    translation.cells, translation.sheets
                )
            },
        )
        .fact(
            self.tr("Made for the game", "Сделан для игры"),
            &translation.game_version,
        );
        let dialog = match &translation.key {
            Some(key) => dialog
                .fact(
                    self.tr("Publisher", "Издатель"),
                    format!("{} {}", self.tr("key", "ключ"), short(key)),
                )
                .text(self.tr(
                    "Install only if you trust this publisher.",
                    "Устанавливайте, только если доверяете этому издателю.",
                )),
            None => dialog.toned(
                self.tr(
                    "This pack is not signed: anyone could have made or changed it.",
                    "Этот пак не подписан: его мог сделать или изменить кто угодно.",
                ),
                Tone::Warning,
            ),
        };
        dialog.row(|row| {
            row.action(
                "install",
                self.tr("Install", "Установить"),
                ActionKind::Main,
                self.job.is_none(),
            )
            .action(
                "cancel",
                self.tr("Not now", "Не сейчас"),
                ActionKind::Quiet,
                true,
            )
        })
    }

    /// The question whether to remove an installed translation.
    fn removal_dialog(&self, index: usize) -> Option<Dialog> {
        let translation = self.library.get(index)?;
        Some(
            Dialog::new("remove", self.tr("Remove this translation?", "Удалить перевод?"))
                .styled(
                    format!("{} {}", translation.title, translation.version),
                    Role::Title,
                    Tone::Normal,
                )
                .text(self.tr(
                    "Its files leave Harmonia; download or add the pack again to get it back.",
                    "Его файлы удаляются из Harmonia; чтобы вернуть перевод, скачайте или добавьте пак снова.",
                ))
                .row(|row| {
                    row.action(
                        "remove-yes",
                        self.tr("Remove", "Удалить"),
                        ActionKind::Danger,
                        self.job.is_none(),
                    )
                    .action("remove-no", self.tr("Cancel", "Отмена"), ActionKind::Quiet, true)
                }),
        )
    }

    /// One question at a time over the window: the publisher of a link
    /// just added, then a checked pack, then a removal, then a group to
    /// delete.
    pub(crate) fn dialog(&self) -> Option<Dialog> {
        self.asking
            .as_deref()
            .and_then(|url| self.trust_dialog(url))
            .or_else(|| {
                self.inspection
                    .as_ref()
                    .map(|inspection| self.inspection_dialog(inspection))
            })
            .or_else(|| self.removing.and_then(|index| self.removal_dialog(index)))
            .or_else(|| self.group_dialog())
    }

    fn click_feed(&mut self, id: &str) -> bool {
        let index = |prefix: &str| {
            id.strip_prefix(prefix)
                .and_then(|i| i.parse::<usize>().ok())
        };
        if let Some(index) = index("feed-remove-") {
            if index < self.settings.feeds.len() {
                let url = self.settings.feeds.remove(index);
                self.feeds.retain(|(feed, _)| *feed != url);
                self.pending.retain(|pending| pending.feed != url);
                for translation in &mut self.library {
                    if translation.feed.as_deref() == Some(url.as_str()) {
                        translation.feed = None;
                    }
                }
                self.save();
            }
        } else if let Some(index) = index("feed-get-") {
            if let Some(url) = self.settings.feeds.get(index).cloned()
                && let Some((_, view)) = self.feeds.iter().find(|(feed, _)| *feed == url)
                && let FeedStatus::Available(release) = &view.status
            {
                let release = (**release).clone();
                self.cancel_inspection();
                if !self.wanted.contains(&url) {
                    self.wanted.push(url.clone());
                }
                self.request(&url, &release);
            }
        } else {
            match id {
                "feed-add" => {
                    let url = self.feed_input.clone();
                    self.add_feed(&url);
                    if self.feed_input.is_empty() {
                        self.adding = false;
                    }
                }
                "suggest-install" => {
                    if !self.wanted.iter().any(|w| w == SUGGESTED_FEED) {
                        self.wanted.push(SUGGESTED_FEED.to_owned());
                    }
                    self.add_feed(SUGGESTED_FEED);
                }
                "feed-open" => self.adding = true,
                "trust" => self.trust(),
                "distrust" => {
                    if let Some(url) = self.asking.take()
                        && let Some(index) = self.settings.feeds.iter().position(|f| *f == url)
                    {
                        self.settings.feeds.remove(index);
                        self.feeds.retain(|(feed, _)| *feed != url);
                        self.wanted.retain(|wanted| *wanted != url);
                        self.save();
                    }
                }
                "feed-close" => {
                    self.adding = false;
                    self.feed_input.clear();
                }
                "feed-check" => {
                    self.next_check = 0.0;
                    self.since_check = 0.0;
                }
                _ => return false,
            }
        }
        true
    }

    /// Handles a click of the Translation tab or of its dialogs; false for
    /// a part it does not have.
    pub(crate) fn translation_click(&mut self, id: &str) -> bool {
        if id.starts_with("row-") {
            self.chosen = if self.chosen.as_deref() == Some(id) {
                None
            } else {
                Some(id.to_owned())
            };
            return true;
        }
        if self.click_feed(id) {
            return true;
        }
        let index = |prefix: &str| {
            id.strip_prefix(prefix)
                .and_then(|i| i.parse::<usize>().ok())
        };
        if let Some(index) = index("check-") {
            if let Some((path, _)) = self.imports.get(index).cloned() {
                self.cancel_inspection();
                self.start_job("inspect", &path, Job::Inspect(None));
            }
        } else if let Some(index) = index("news-") {
            let hash = self.library.get(index).map(|t| t.hash.clone());
            self.news = if self.news == hash { None } else { hash };
        } else if let Some(index) = index("use-") {
            if let Some(hash) = self.library.get(index).map(|t| t.hash.clone()) {
                self.settings.active = Some(hash.clone());
                self.save();
                if self.table_is_current(&hash) {
                    self.say(
                        words(
                            "Applies at the next game start.",
                            "Применится при следующем запуске игры.",
                        ),
                        Tone::Normal,
                    );
                } else {
                    self.compile(&hash);
                }
            }
        } else if let Some(index) = index("remove-") {
            self.removing = Some(index);
        } else {
            match id {
                "remove-yes" => self.remove_chosen(),
                "none" | "use-none" => {
                    self.settings.active = None;
                    self.save();
                    self.say(
                        words(
                            "The game stays untranslated from the next start.",
                            "Со следующего запуска игра будет без перевода.",
                        ),
                        Tone::Normal,
                    );
                }
                "remove-no" => self.removing = None,
                "refresh" => self.imports = library::imports(),
                "install" => self.install_inspected(),
                "cancel" => self.cancel_inspection(),
                _ => return false,
            }
        }
        true
    }

    fn remove_chosen(&mut self) {
        let Some(index) = self.removing.take() else {
            return;
        };
        if index >= self.library.len() {
            return;
        }
        let removed = self.library.remove(index);
        if self.settings.active.as_deref() == Some(removed.hash.as_str()) {
            self.settings.active = None;
        }
        if self.applied.as_deref() != Some(removed.hash.as_str()) {
            library::remove_files(&removed.hash);
        }
        self.chosen = None;
        self.save();
        self.say(
            words(
                format!("Removed {}.", removed.title),
                format!("Удалён {}.", removed.title),
            ),
            Tone::Normal,
        );
    }
}
