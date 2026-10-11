//! Harmonia for Angelica: applies Harmonia translation packs (`.hpk`) to the
//! game's text.
//!
//! The player puts a pack into the plugin's `import` folder; a job verifies
//! it completely (digest, signature, every record and string) and, after
//! the player confirms the publisher, installs it. Choosing a translation
//! builds its text table in a job, which also adds the pack's glyphs to the
//! game's own font files. At the next game start Harmonia activates the
//! table, so the host translates every row the game builds, has the host
//! serve the patched font files instead of the game's, and adds Cyrillic
//! letters to the game's letter case when the translation is written in
//! Cyrillic.
//!
//! A translation can also come from its publisher's feed: Harmonia checks
//! the feed links the player added, downloads a new release in a job,
//! verifies it like an imported pack, and installs it. The first release of
//! a publisher waits for the player's confirmation; later ones signed by
//! the same key install by themselves.
//!
//! Like the Dalamud build, Harmonia changes translations only at game
//! start: the game copies text into its own caches, so a change in the
//! middle of a session would show a mix.
pub mod atlas;
pub mod compile;
pub mod fdt;
pub mod feed;
pub mod font_patch;
pub mod fonts;
pub mod library;
pub mod pack;

use angelica_sdk::{
    Plugin,
    api::{excel_text, file_replacements, game_files, jobs, runtime, storage, text_case},
    http,
    ui::{ActionKind, Entry, Event, Mark, Tone, Window},
};
use library::{Settings, Stamp, Translation};
use serde::{Deserialize, Serialize};

const TABLE: &str = "translation";
const WINDOW: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inspection {
    path: String,
    translation: Translation,
    previous_key: Option<String>,
    /// The feed the pack was downloaded from.
    #[serde(default)]
    feed: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct InstallInput {
    path: String,
    hash: String,
    feed: Option<String>,
}

/// What a feed offers, or why it could not be read.
#[derive(Serialize, Deserialize)]
struct Checked {
    url: String,
    title: Option<String>,
    release: Option<feed::Release>,
    error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct DownloadInput {
    feed: String,
    release: feed::Release,
}

enum FeedStatus {
    UpToDate,
    Available(Box<feed::Release>),
    Problem(String),
}

struct FeedView {
    title: Option<String>,
    status: FeedStatus,
}

/// Seconds after the start before the first check of the feeds, and between
/// checks.
const FIRST_CHECK: f64 = 30.0;
const CHECK_EVERY: f64 = 3600.0;

#[derive(Serialize, Deserialize)]
struct CompileInput {
    hash: String,
    kept_sheets: Vec<String>,
}

enum Job {
    Inspect,
    Feeds,
    Download,
    Install,
    Compile(String),
}

struct Harmonia {
    open: bool,
    library: Vec<Translation>,
    settings: Settings,
    /// The translation applied in this session.
    applied: Option<String>,
    started_with_game: bool,
    imports: Vec<(String, u64)>,
    inspection: Option<Inspection>,
    job: Option<(u64, Job)>,
    progress: (f32, String),
    message: (String, Tone),
    status: Option<excel_text::TableStatus>,
    since_status: f64,
    /// Font files served in this session.
    fonts: usize,
    /// What the last check found, by feed link.
    feeds: Vec<(String, FeedView)>,
    feed_input: String,
    since_check: f64,
    next_check: f64,
    /// Releases waiting to be downloaded.
    pending: Vec<DownloadInput>,
    /// The id of the chosen row.
    chosen: Option<String>,
    /// Whether Angelica's interface is in Russian.
    russian: bool,
}

fn short(fingerprint: &str) -> String {
    fingerprint
        .as_bytes()
        .chunks(4)
        .take(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn megabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64 / 1_048_576.0;
    format!("{value:.1} MB")
}

impl Harmonia {
    fn say(&mut self, text: impl Into<String>, tone: Tone) {
        self.message = (text.into(), tone);
        let text = &self.message.0;
        match tone {
            Tone::Bad => angelica_sdk::error(text),
            Tone::Warning => angelica_sdk::warning(text),
            _ => angelica_sdk::info(text),
        }
    }

    fn translation(&self, hash: &str) -> Option<&Translation> {
        self.library.iter().find(|t| t.hash == hash)
    }

    /// Whether the table of `hash` was built with the current settings and
    /// its fonts from the running game's files.
    fn table_is_current(&self, hash: &str) -> bool {
        matches!(
            library::load_stamp(hash),
            Ok(Some(stamp)) if stamp.kept_sheets == self.settings.kept_sheets
                && stamp.game_version == runtime::game_version()
        )
    }

    /// Applies the chosen translation at game start.
    fn apply(&mut self, hash: &str) -> Result<(), String> {
        let translation = self
            .translation(hash)
            .cloned()
            .ok_or("the chosen translation is no longer installed")?;
        if !self.table_is_current(hash) {
            self.compile(hash);
            return Err(
                "the translation is being prepared; it applies at the next game start".into(),
            );
        }
        let info =
            excel_text::activate(TABLE, &library::table_path(hash), 0).map_err(|e| e.message)?;
        self.applied = Some(hash.to_owned());
        self.say(
            format!(
                "{} {}: {} cells in {} sheets ready; the game translates them as it loads its text.",
                translation.title, translation.version, info.cells, info.sheets
            ),
            Tone::Good,
        );
        if let Ok(Some(stamp)) = library::load_stamp(hash) {
            self.replace_fonts(&stamp);
        }
        if compile::is_cyrillic(&translation.language) {
            let letters: Vec<_> = compile::cyrillic_letters()
                .into_iter()
                .map(|(lower, upper)| text_case::Letter { lower, upper })
                .collect();
            if let Err(error) = text_case::set_letters(&letters) {
                self.say(
                    format!("Capital letters stay lowercase: {}", error.message),
                    Tone::Warning,
                );
            }
        }
        Ok(())
    }

    /// Serves the translation's font files; a failure leaves the game's own
    /// fonts and is reported.
    fn replace_fonts(&mut self, stamp: &Stamp) {
        let mut failed = Vec::new();
        for (game_path, storage_path) in &stamp.fonts {
            if let Err(error) = file_replacements::replace(game_path, storage_path) {
                failed.push(format!("{game_path}: {}", error.message));
            }
        }
        self.fonts = if failed.is_empty() {
            stamp.fonts.len()
        } else {
            self.say(
                format!("The game's own fonts stay: {}", failed.join("; ")),
                Tone::Warning,
            );
            0
        };
    }

    fn start_job(&mut self, name: &str, input: &impl Serialize, job: Job) {
        if self.job.is_some() {
            self.say("Wait for the running task to finish.", Tone::Warning);
            return;
        }
        let input = match serde_json::to_vec(input) {
            Ok(input) => input,
            Err(error) => return self.say(error.to_string(), Tone::Bad),
        };
        match jobs::start(name, &input) {
            Ok(id) => {
                self.job = Some((id, job));
                self.progress = (0.0, String::new());
            }
            Err(error) => self.say(format!("Cannot start: {}", error.message), Tone::Bad),
        }
    }

    fn compile(&mut self, hash: &str) {
        let input = CompileInput {
            hash: hash.to_owned(),
            kept_sheets: self.settings.kept_sheets.clone(),
        };
        self.start_job("compile", &input, Job::Compile(hash.to_owned()));
    }

    fn save(&mut self) {
        let result = library::save_library(&self.library)
            .and_then(|()| library::save_settings(&self.settings));
        if let Err(error) = result {
            self.say(format!("Cannot save: {error}"), Tone::Bad);
        }
    }

    fn finished(&mut self, job: Job, output: &[u8]) -> Result<(), String> {
        match job {
            Job::Inspect => {
                let inspection: Inspection =
                    serde_json::from_slice(output).map_err(|e| e.to_string())?;
                if self.translation(&inspection.translation.hash).is_some() {
                    self.say("This pack is already installed.", Tone::Muted);
                } else {
                    self.inspection = Some(inspection);
                }
            }
            Job::Install => {
                let translation: Translation =
                    serde_json::from_slice(output).map_err(|e| e.to_string())?;
                self.say(
                    format!("Installed {} {}.", translation.title, translation.version),
                    Tone::Good,
                );
                self.installed(translation);
            }
            Job::Feeds => {
                let checked: Vec<Checked> =
                    serde_json::from_slice(output).map_err(|e| e.to_string())?;
                for checked in checked {
                    self.evaluate(checked);
                }
            }
            Job::Download => {
                let inspection: Inspection =
                    serde_json::from_slice(output).map_err(|e| e.to_string())?;
                self.downloaded(inspection);
            }
            Job::Compile(hash) => {
                let stamp: Stamp = serde_json::from_slice(output).map_err(|e| e.to_string())?;
                let ready = if self.applied.as_deref() == Some(hash.as_str())
                    && self.settings.active.as_deref() == Some(&hash)
                {
                    "Prepared again; the change applies at the next game start."
                } else {
                    "Ready; it applies at the next game start."
                };
                let reserved = if stamp.summary.reserved > 0 {
                    format!(
                        " {} cells hold reserved game tokens and are not written.",
                        stamp.summary.reserved
                    )
                } else {
                    String::new()
                };
                self.say(
                    format!(
                        "{ready} {} cells in {} sheets.{reserved}",
                        stamp.summary.cells, stamp.summary.sheets
                    ),
                    Tone::Good,
                );
            }
        }
        Ok(())
    }

    /// Adds an installed translation. A release from a feed replaces the
    /// versions the feed installed before, and an imported pack of the same
    /// publisher and title.
    fn installed(&mut self, translation: Translation) {
        let mut chosen = false;
        if let Some(url) = &translation.feed {
            let outdated = |old: &Translation| {
                old.feed.as_deref() == Some(url.as_str())
                    || (old.feed.is_none()
                        && old.key.is_some()
                        && old.key == translation.key
                        && old.title == translation.title)
            };
            let (replaced, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.library)
                .into_iter()
                .partition(outdated);
            self.library = kept;
            for old in replaced {
                chosen |= self.settings.active.as_deref() == Some(old.hash.as_str());
                if self.applied.as_deref() != Some(old.hash.as_str()) {
                    library::remove_files(&old.hash);
                }
            }
            self.set_feed(url, None, FeedStatus::UpToDate);
        }
        let hash = translation.hash.clone();
        self.library.push(translation);
        self.imports = library::imports();
        if chosen {
            self.settings.active = Some(hash.clone());
        }
        self.save();
        if chosen {
            self.compile(&hash);
        }
    }

    fn set_feed(&mut self, url: &str, title: Option<String>, status: FeedStatus) {
        if let Some((_, view)) = self.feeds.iter_mut().find(|(feed, _)| feed == url) {
            view.status = status;
            if title.is_some() {
                view.title = title;
            }
        } else {
            self.feeds
                .push((url.to_owned(), FeedView { title, status }));
        }
    }

    /// Decides what a checked feed means for the installed translations. A
    /// newer release of a translation the feed already installed downloads
    /// by itself; a feed's first release waits for the player.
    fn evaluate(&mut self, checked: Checked) {
        let Checked {
            url,
            title,
            release,
            error,
        } = checked;
        if !self.settings.feeds.contains(&url) {
            return;
        }
        let Some(release) = release else {
            let reason = error.unwrap_or_else(|| "The feed has no stable release.".into());
            return self.set_feed(&url, title, FeedStatus::Problem(reason));
        };
        // A pack the player imported by hand is the feed's release.
        if let Some(same) = self
            .library
            .iter_mut()
            .find(|t| t.hash == release.pack_hash && t.feed.is_none())
        {
            same.feed = Some(url.clone());
            self.save();
        }
        let installed = self
            .library
            .iter()
            .filter(|t| t.feed.as_deref() == Some(url.as_str()))
            .map(|t| t.version.as_str())
            .max();
        let status = if compile::game_language(&release.game_language).is_none() {
            FeedStatus::Problem(format!(
                "The release is made for the game language {}, which the game does not have.",
                release.game_language
            ))
        } else if !release.is_newer_than(installed) {
            FeedStatus::UpToDate
        } else if !release.supported_by(env!("CARGO_PKG_VERSION")) {
            FeedStatus::Problem("The newest release needs a newer Harmonia.".into())
        } else {
            if installed.is_some() {
                self.request(&url, &release);
            }
            FeedStatus::Available(Box::new(release))
        };
        self.set_feed(&url, title, status);
    }

    fn request(&mut self, url: &str, release: &feed::Release) {
        if !self.pending.iter().any(|pending| pending.feed == url) {
            self.pending.push(DownloadInput {
                feed: url.to_owned(),
                release: release.clone(),
            });
        }
    }

    /// A release was downloaded and verified. It installs at once when the
    /// key that signed it is the key of the translation it replaces;
    /// anything else is the player's decision.
    fn downloaded(&mut self, inspection: Inspection) {
        let translation = &inspection.translation;
        if self.translation(&translation.hash).is_some() {
            let _ = storage::remove(&inspection.path);
            return self.say("This pack is already installed.", Tone::Muted);
        }
        let trusted = translation.key.is_some()
            && self
                .library
                .iter()
                .any(|old| old.feed == inspection.feed && old.key == translation.key);
        if trusted {
            let input = InstallInput {
                path: inspection.path.clone(),
                hash: translation.hash.clone(),
                feed: inspection.feed.clone(),
            };
            self.start_job("install", &input, Job::Install);
        } else {
            self.say(
                format!(
                    "{} {} is downloaded and waits for your decision in Harmonia.",
                    translation.title, translation.version
                ),
                Tone::Warning,
            );
            self.inspection = Some(inspection);
        }
    }

    fn add_feed(&mut self) {
        let url = self.feed_input.trim().to_owned();
        if !feed::is_link(&url) {
            return self.say(
                "This does not look like a link. It must start with https://",
                Tone::Warning,
            );
        }
        if self.settings.feeds.contains(&url) {
            return self.say("This link is already added.", Tone::Muted);
        }
        self.settings.feeds.push(url);
        self.feed_input.clear();
        self.save();
        self.next_check = 0.0;
    }

    /// Drops what waits for the player; a downloaded pack is removed.
    fn cancel_inspection(&mut self) {
        if let Some(inspection) = self.inspection.take()
            && inspection.feed.is_some()
        {
            let _ = storage::remove(&inspection.path);
        }
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
                self.save();
            }
        } else if let Some(index) = index("feed-get-") {
            if let Some(url) = self.settings.feeds.get(index).cloned()
                && let Some((_, view)) = self.feeds.iter().find(|(feed, _)| *feed == url)
                && let FeedStatus::Available(release) = &view.status
            {
                let release = (**release).clone();
                self.cancel_inspection();
                self.request(&url, &release);
            }
        } else {
            match id {
                "feed-add" => self.add_feed(),
                "feed-check" => self.next_check = 0.0,
                _ => return false,
            }
        }
        true
    }

    fn click(&mut self, id: &str) {
        if id.starts_with("row-") {
            self.chosen = if self.chosen.as_deref() == Some(id) {
                None
            } else {
                Some(id.to_owned())
            };
            return;
        }
        if self.click_feed(id) {
            return;
        }
        if let Some(index) = id
            .strip_prefix("check-")
            .and_then(|i| i.parse::<usize>().ok())
        {
            if let Some((path, _)) = self.imports.get(index).cloned() {
                self.cancel_inspection();
                self.start_job("inspect", &path, Job::Inspect);
            }
        } else if let Some(index) = id
            .strip_prefix("use-")
            .and_then(|i| i.parse::<usize>().ok())
        {
            if let Some(hash) = self.library.get(index).map(|t| t.hash.clone()) {
                self.settings.active = Some(hash.clone());
                self.save();
                if self.table_is_current(&hash) {
                    self.say("Applies at the next game start.", Tone::Normal);
                } else {
                    self.compile(&hash);
                }
            }
        } else if let Some(index) = id
            .strip_prefix("remove-")
            .and_then(|i| i.parse::<usize>().ok())
        {
            if index < self.library.len() {
                let removed = self.library.remove(index);
                if self.settings.active.as_deref() == Some(removed.hash.as_str()) {
                    self.settings.active = None;
                }
                if self.applied.as_deref() != Some(removed.hash.as_str()) {
                    library::remove_files(&removed.hash);
                }
                self.save();
                self.say(format!("Removed {}.", removed.title), Tone::Normal);
            }
        } else {
            match id {
                "none" => {
                    self.settings.active = None;
                    self.save();
                    self.say(
                        "The game stays untranslated from the next start.",
                        Tone::Normal,
                    );
                }
                "refresh" => self.imports = library::imports(),
                "install" => {
                    if self.job.is_some() {
                        self.say("Wait for the running task to finish.", Tone::Warning);
                    } else if let Some(inspection) = self.inspection.take() {
                        let input = InstallInput {
                            path: inspection.path,
                            hash: inspection.translation.hash,
                            feed: inspection.feed,
                        };
                        self.start_job("install", &input, Job::Install);
                    }
                }
                "cancel" => self.cancel_inspection(),
                _ => {}
            }
        }
    }

    /// The title bar's line: what waits for a game start, else what is
    /// applied.
    fn status_line(&self) -> Option<(String, Tone)> {
        let name = |hash: &Option<String>| {
            hash.as_deref()
                .and_then(|hash| self.translation(hash))
                .map(|t| format!("{} · {}", t.title, t.language))
        };
        if self.applied != self.settings.active {
            let text = match name(&self.settings.active) {
                Some(chosen) => format!("{chosen} — at the next game start"),
                None => "No translation from the next game start".into(),
            };
            return Some((text, Tone::Warning));
        }
        name(&self.applied).map(|applied| (applied, Tone::Good))
    }

    /// `en` or `ru`, by the language of Angelica's interface.
    fn tr<'a>(&self, en: &'a str, ru: &'a str) -> &'a str {
        if self.russian { ru } else { en }
    }

    /// What the last check of `url` found, in words, and whether it is a
    /// problem.
    fn feed_state(&self, url: &str) -> (String, bool) {
        let view = self
            .feeds
            .iter()
            .find(|(feed, _)| feed == url)
            .map(|(_, view)| &view.status);
        match view {
            None => (
                self.tr("Not checked yet.", "Ещё не проверялось.")
                    .to_owned(),
                false,
            ),
            Some(FeedStatus::UpToDate) => (
                self.tr("Up to date.", "Актуальная версия.").to_owned(),
                false,
            ),
            Some(FeedStatus::Available(release)) => (
                if self.russian {
                    format!("Доступна версия {}.", release.version)
                } else {
                    format!("Version {} is available.", release.version)
                },
                false,
            ),
            Some(FeedStatus::Problem(reason)) => (reason.clone(), true),
        }
    }

    /// What the applied translation did in this session.
    fn session_facts(&self, mut entry: Entry) -> Entry {
        if let Some(status) = &self.status {
            entry = entry.fact(
                self.tr("Translated so far", "Переведено в этой игре"),
                if self.russian {
                    format!(
                        "строк: {}, ячеек: {}. Оставлено как в игре, потому что текст игры изменился: {}.",
                        status.rows_changed, status.cells_replaced, status.guard_mismatches
                    )
                } else {
                    format!(
                        "{} rows, {} cells. Left in the original because the game's text changed: {}.",
                        status.rows_changed, status.cells_replaced, status.guard_mismatches
                    )
                },
            );
            if status.rows_refused > 0 {
                entry = entry.fact(
                    self.tr("Left untouched", "Не тронуто"),
                    if self.russian {
                        format!("строк с неожиданным устройством: {}.", status.rows_refused)
                    } else {
                        format!("{} rows with an unexpected layout.", status.rows_refused)
                    },
                );
            }
            if !status.sheets_mismatched.is_empty() {
                let names: Vec<_> = status
                    .sheets_mismatched
                    .iter()
                    .take(8)
                    .map(|m| m.sheet.as_str())
                    .collect();
                entry = entry.fact(
                    self.tr("Differ in this game build", "Отличаются в этой сборке игры"),
                    format!("{} ({})", status.sheets_mismatched.len(), names.join(", ")),
                );
            }
        }
        if self.fonts > 0 {
            entry = entry.fact(
                self.tr("Fonts", "Шрифты"),
                if self.russian {
                    format!("файлов шрифтов игры с буквами перевода: {}.", self.fonts)
                } else {
                    format!(
                        "{} game font files with the translation's glyphs.",
                        self.fonts
                    )
                },
            );
        }
        entry
    }

    /// An installed translation as a row; the chosen one shows what it is
    /// and what can be done with it.
    fn translation_entry(&self, index: usize, translation: &Translation) -> Entry {
        let id = format!("row-t{index}");
        let hash = translation.hash.as_str();
        let applied = self.applied.as_deref() == Some(hash);
        let active = self.settings.active.as_deref() == Some(hash);
        let idle = self.job.is_none();
        let mark = match (applied, active) {
            (true, true) => Mark::On,
            (false, false) => Mark::Off,
            _ => Mark::Attention,
        };
        let chosen = self.chosen.as_deref() == Some(id.as_str());
        let mut entry = Entry::new(&id, &translation.title)
            .note(&translation.language)
            .value(&translation.version)
            .mark(mark)
            .chosen(chosen);
        if !active && idle {
            entry = entry.action(format!("use-{index}"), self.tr("Use", "Использовать"));
        }
        if !chosen {
            return entry;
        }
        if applied != active {
            entry = entry.fact(
                self.tr("State", "Состояние"),
                if active {
                    self.tr(
                        "Applies at the next game start.",
                        "Применится при следующем запуске игры.",
                    )
                } else {
                    self.tr(
                        "In use until the game closes.",
                        "Используется, пока игра не закрыта.",
                    )
                },
            );
        }
        let feed = translation.feed.as_deref();
        entry = entry
            .fact(self.tr("Team", "Команда"), &translation.team)
            .fact(
                self.tr("Translation", "Перевод"),
                format!(
                    "{} ← {}, {} {}",
                    translation.language,
                    translation.game_language,
                    translation.cells,
                    self.tr("cells", "ячеек")
                ),
            )
            .fact(
                self.tr("Made for game", "Для игры"),
                &translation.game_version,
            )
            .fact(
                self.tr("Publisher", "Издатель"),
                translation.key.as_deref().map_or_else(
                    || self.tr("not signed", "без подписи").to_owned(),
                    |key| format!("{} {}", self.tr("key", "ключ"), short(key)),
                ),
            )
            .fact(
                self.tr("Updates", "Обновления"),
                feed.map_or_else(
                    || {
                        self.tr(
                            "No link; updated from a file.",
                            "Нет ссылки; обновляется из файла.",
                        )
                        .to_owned()
                    },
                    |url| self.feed_state(url).0,
                ),
            );
        if applied {
            entry = self.session_facts(entry);
        }
        self.translation_actions(entry, index, translation)
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
        let feed = translation.feed.as_deref();
        if active {
            entry = entry.more(
                "none",
                self.tr("Stop using", "Не использовать"),
                ActionKind::Normal,
                idle,
            );
        }
        if let Some(position) =
            feed.and_then(|url| self.settings.feeds.iter().position(|f| f == url))
        {
            entry = entry.more(
                format!("feed-remove-{position}"),
                self.tr("Forget the link", "Забыть ссылку"),
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

    /// A feed nothing is installed from yet.
    fn feed_entry(&self, index: usize, url: &str) -> Entry {
        let id = format!("row-f{index}");
        let view = self.feeds.iter().find(|(feed, _)| feed == url);
        let title = view
            .and_then(|(_, view)| view.title.as_deref())
            .unwrap_or(url);
        let (state, problem) = self.feed_state(url);
        let chosen = self.chosen.as_deref() == Some(id.as_str());
        let mut entry = Entry::new(&id, title)
            .note(state)
            .mark(if problem { Mark::Attention } else { Mark::None })
            .chosen(chosen);
        let waits = self.pending.iter().any(|pending| pending.feed == url);
        if matches!(view, Some((_, view)) if matches!(view.status, FeedStatus::Available(_)))
            && !waits
            && self.job.is_none()
        {
            entry = entry.action(format!("feed-get-{index}"), self.tr("Download", "Скачать"));
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

    /// The pack that waits for the player's decision.
    fn inspection_lines(&self, window: Window, inspection: &Inspection) -> Window {
        let translation = &inspection.translation;
        let mut window = window
            .heading(format!("{} {}", translation.title, translation.version))
            .text(if self.russian {
                format!(
                    "{} — перевод на {} с {}, ячеек: {}, листов: {}, сделан для игры {}.",
                    translation.team,
                    translation.language,
                    translation.game_language,
                    translation.cells,
                    translation.sheets,
                    translation.game_version
                )
            } else {
                format!(
                    "{} — {} translation from {}, {} cells in {} sheets, made for game {}.",
                    translation.team,
                    translation.language,
                    translation.game_language,
                    translation.cells,
                    translation.sheets,
                    translation.game_version
                )
            });
        window = match &translation.key {
            Some(key) if self.russian => window.text(format!(
                "Подписан ключом {}. Устанавливайте, только если доверяете этому издателю.",
                short(key)
            )),
            Some(key) => window.text(format!(
                "Signed by key {}. Install only if you trust this publisher.",
                short(key)
            )),
            None => window.toned(
                self.tr(
                    "This pack is not signed: anyone could have made or changed it.",
                    "Этот пак не подписан: его мог сделать или изменить кто угодно.",
                ),
                Tone::Warning,
            ),
        };
        window
            .row(|row| {
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
            .separator()
    }

    /// The installed translations, in use first, then the feeds nothing is
    /// installed from and the files waiting in the import folder.
    fn rows(&self, mut window: Window) -> Window {
        let in_use = |t: &Translation| {
            self.applied.as_deref() == Some(t.hash.as_str())
                || self.settings.active.as_deref() == Some(t.hash.as_str())
        };
        for (used, title) in [
            (true, self.tr("In use", "Используется")),
            (false, self.tr("Installed", "Установлены")),
        ] {
            let mut first = true;
            for (index, translation) in self.library.iter().enumerate() {
                if in_use(translation) != used {
                    continue;
                }
                if std::mem::take(&mut first) {
                    window = window.heading(title);
                }
                window = window.entry(self.translation_entry(index, translation));
            }
        }
        let mut first = true;
        for (index, url) in self.settings.feeds.iter().enumerate() {
            if self
                .library
                .iter()
                .any(|t| t.feed.as_deref() == Some(url.as_str()))
            {
                continue;
            }
            if std::mem::take(&mut first) {
                window = window.heading(self.tr("By link", "По ссылке"));
            }
            window = window.entry(self.feed_entry(index, url));
        }
        if !self.imports.is_empty() {
            window = window.heading(self.tr("Files", "Файлы"));
        }
        for (index, (path, size)) in self.imports.iter().enumerate() {
            let name = path.rsplit('/').next().unwrap_or(path);
            let mut entry = Entry::new(format!("row-i{index}"), name).note(megabytes(*size));
            if self.job.is_none() {
                entry = entry.action(format!("check-{index}"), self.tr("Check", "Проверить"));
            }
            window = window.entry(entry);
        }
        window
    }
}

impl Plugin for Harmonia {
    fn start() -> Result<Self, String> {
        let mut plugin = Self {
            open: false,
            library: library::load_library()?,
            settings: library::load_settings()?,
            applied: None,
            started_with_game: runtime::started_with_game(),
            imports: library::imports(),
            inspection: None,
            job: None,
            progress: (0.0, String::new()),
            message: (String::new(), Tone::Muted),
            status: None,
            since_status: f64::INFINITY,
            fonts: 0,
            feeds: Vec::new(),
            feed_input: String::new(),
            since_check: 0.0,
            next_check: FIRST_CHECK,
            pending: Vec::new(),
            chosen: None,
            russian: runtime::language() == "ru",
        };
        let _ = storage::remove(feed::DOWNLOAD);
        if plugin.started_with_game
            && let Some(hash) = plugin.settings.active.clone()
            && let Err(reason) = plugin.apply(&hash)
        {
            plugin.say(
                format!("No translation in this session: {reason}"),
                Tone::Warning,
            );
        }
        Ok(plugin)
    }

    fn tick(&mut self, elapsed: f64) -> Result<(), String> {
        if let Some((id, _)) = &self.job {
            match jobs::poll(*id).map_err(|e| e.message)? {
                jobs::State::Running(progress) => {
                    self.progress = (progress.fraction, progress.message);
                }
                jobs::State::Finished(output) => {
                    let (_, job) = self.job.take().expect("a running job");
                    if let Err(error) = self.finished(job, &output) {
                        self.say(error, Tone::Bad);
                    }
                }
                jobs::State::Failed(reason) => {
                    self.job = None;
                    self.say(reason, Tone::Bad);
                }
            }
        }
        self.since_check += elapsed;
        if self.job.is_none() {
            if let Some(input) = self.pending.pop() {
                self.start_job("download", &input, Job::Download);
            } else if !self.settings.feeds.is_empty() && self.since_check >= self.next_check {
                self.since_check = 0.0;
                self.next_check = CHECK_EVERY;
                let feeds = self.settings.feeds.clone();
                self.start_job("feeds", &feeds, Job::Feeds);
            }
        }
        self.since_status += elapsed;
        if self.applied.is_some() && self.since_status >= 1.0 {
            self.since_status = 0.0;
            self.status = excel_text::status(TABLE).ok();
        }
        Ok(())
    }

    fn ui(&mut self) -> Vec<Window> {
        if !self.open {
            return Vec::new();
        }
        self.russian = runtime::language() == "ru";
        let mut window = Window::new(WINDOW, "Harmonia");
        if let Some((text, tone)) = self.status_line() {
            window = window.status(text, tone);
        }
        if let Some(inspection) = &self.inspection {
            window = self.inspection_lines(window, inspection);
        }
        if !self.started_with_game {
            window = window.toned(
                self.tr(
                    "Harmonia was enabled after the game started; translations apply when the game starts.",
                    "Harmonia включена после запуска игры; переводы применяются при запуске игры.",
                ),
                Tone::Warning,
            );
        }
        if self.library.is_empty() && self.settings.feeds.is_empty() && self.imports.is_empty() {
            window =
                window.muted(self.tr("No translation is installed.", "Переводы не установлены."));
        } else if self.applied.is_none() {
            window = window.muted(self.tr(
                "No translation is applied in this session.",
                "В этой игре перевод не применён.",
            ));
        }
        window = self.rows(window);
        if self.job.is_some() {
            window = window.progress(self.progress.0, &self.progress.1);
        }
        if !self.message.0.is_empty() {
            window = window.toned(&self.message.0, self.message.1);
        }
        window = window
            .separator()
            .input(
                "feed",
                "",
                &self.feed_input,
                self.tr("Link to a translation", "Ссылка на перевод"),
            )
            .row(|row| {
                row.action(
                    "feed-add",
                    self.tr("Add", "Добавить"),
                    ActionKind::Normal,
                    true,
                )
                .action(
                    "feed-check",
                    self.tr("Check for updates", "Проверить обновления"),
                    ActionKind::Quiet,
                    !self.settings.feeds.is_empty() && self.job.is_none(),
                )
                .action(
                    "refresh",
                    self.tr("Look for files", "Найти файлы"),
                    ActionKind::Quiet,
                    true,
                )
                .tooltip(self.tr(
                    "A .hpk file goes into the import folder of Harmonia's folder: the window's menu, Open folder.",
                    "Файл .hpk кладётся в папку import в папке Harmonia: меню окна, «Открыть папку».",
                ))
            });
        vec![window]
    }

    fn ui_event(&mut self, event: Event) {
        match event {
            Event::OpenRequested => {
                self.open = true;
                self.imports = library::imports();
            }
            Event::Closed(_) => self.open = false,
            Event::Clicked(target) => self.click(&target.id),
            Event::Edited(edited) if edited.id == "feed" => self.feed_input = edited.value,
            _ => {}
        }
    }

    fn run_job(name: &str, input: Vec<u8>) -> Result<Vec<u8>, String> {
        let output = match name {
            "inspect" => {
                let path: String = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                serde_json::to_vec(&inspect(&path)?)
            }
            "install" => {
                let input: InstallInput =
                    serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                let inspection = inspect(&input.path)?;
                if inspection.translation.hash != input.hash {
                    return Err("the file changed after it was checked; check it again".into());
                }
                storage::rename(&input.path, &inspection.translation.pack_path())
                    .map_err(|e| e.message)?;
                let mut translation = inspection.translation;
                translation.feed = input.feed;
                serde_json::to_vec(&translation)
            }
            "feeds" => {
                let feeds: Vec<String> =
                    serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                let checked: Vec<Checked> = feeds.into_iter().map(check_feed).collect();
                serde_json::to_vec(&checked)
            }
            "download" => {
                let input: DownloadInput =
                    serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                serde_json::to_vec(&download(&input)?)
            }
            "compile" => {
                let input: CompileInput =
                    serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                let bytes = library::read_file(&library::pack_path(&input.hash), pack::MAX_PACK)?;
                let _ = jobs::report(0.4, "Checking the pack");
                let pack = pack::Pack::parse(&bytes).map_err(|e| e.0)?;
                if pack.hash_hex() != input.hash {
                    return Err("the installed pack changed; install it again".into());
                }
                let _ = jobs::report(0.6, "Preparing the translation");
                let (table, summary) = compile::compile(&pack, &input.kept_sheets)?;
                let _ = jobs::report(0.8, "Preparing fonts");
                let (fonts, font_reports) = prepare_fonts(&pack, &input.hash)?;
                let _ = jobs::report(0.9, "Saving");
                storage::write(&library::table_path(&input.hash), &table).map_err(|e| e.message)?;
                let stamp = Stamp {
                    hash: input.hash,
                    kept_sheets: input.kept_sheets,
                    summary,
                    game_version: runtime::game_version(),
                    fonts,
                    font_reports,
                };
                library::save_stamp(&stamp)?;
                serde_json::to_vec(&stamp)
            }
            _ => return Err(format!("no job {name}")),
        };
        output.map_err(|e| e.to_string())
    }
}

/// Patches the game's font files with the pack's glyphs and saves the
/// changed files in storage.
/// Patched font files, as game path and storage path, and a report per
/// target.
type PreparedFonts = (Vec<(String, String)>, Vec<font_patch::Report>);

fn prepare_fonts(pack: &pack::Pack<'_>, hash: &str) -> Result<PreparedFonts, String> {
    if pack.fonts.is_none() && pack.font_replacements.is_none() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut failure = None;
    let mut read = |path: &str| match game_files::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            if error.kind != angelica_sdk::ErrorKind::NotFound {
                failure = Some(format!("{path}: {}", error.message));
            }
            None
        }
    };
    let patched = font_patch::patch(
        pack.fonts.as_ref(),
        pack.font_replacements.as_ref(),
        &mut read,
    )?;
    if let Some(failure) = failure {
        return Err(failure);
    }
    let mut files = Vec::new();
    for (game_path, contents) in &patched.files {
        let storage_path = library::font_path(hash, game_path);
        storage::write(&storage_path, contents).map_err(|e| e.message)?;
        files.push((game_path.clone(), storage_path));
    }
    Ok((files, patched.reports))
}

/// Reads a feed and picks its newest release.
fn check_feed(url: String) -> Checked {
    let feed = http::get(&url)
        .map_err(|error| format!("The feed could not be read: {}", error.message))
        .and_then(|response| match response.head.status {
            200..=299 => feed::parse(&response.body).map_err(|reason| {
                let mut reason = reason;
                if let Some(first) = reason.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                format!("{reason}.")
            }),
            404 | 410 => Err("Nothing is published at this link.".into()),
            status => Err(format!("The server answered {status}.")),
        });
    match feed {
        Ok(feed) => Checked {
            url,
            release: feed.newest().cloned(),
            title: feed.title,
            error: None,
        },
        Err(error) => Checked {
            url,
            title: None,
            release: None,
            error: Some(error),
        },
    }
}

/// Downloads a release, verifies the file and the pack in it against the
/// feed, and leaves the pack in storage for the install job.
fn download(input: &DownloadInput) -> Result<Inspection, String> {
    let release = &input.release;
    let _ = jobs::report(0.0, "Downloading");
    let (head, _) = http::download(&release.url, feed::DOWNLOAD).map_err(|e| e.message)?;
    if !head.is_success() {
        return Err(format!(
            "the download failed: the server answered {}",
            head.status
        ));
    }
    let file = library::read_file(feed::DOWNLOAD, release.size);
    let _ = storage::remove(feed::DOWNLOAD);
    let _ = jobs::report(0.4, "Unpacking");
    let bytes = feed::unpack(release, file?)?;
    let _ = jobs::report(0.6, "Checking the pack");
    let path = format!("downloads/{}.hpk", release.pack_hash);
    let pack = pack::Pack::parse(&bytes).map_err(|e| e.0)?;
    feed::check_pack(release, &pack)?;
    let mut inspection = describe(&path, &pack)?;
    inspection.feed = Some(input.feed.clone());
    let _ = jobs::report(0.9, "Saving");
    storage::write(&path, &bytes).map_err(|e| e.message)?;
    Ok(inspection)
}

/// Verifies a pack completely and describes it.
fn inspect(path: &str) -> Result<Inspection, String> {
    let bytes = library::read_file(path, pack::MAX_PACK)?;
    let _ = jobs::report(0.4, "Checking the pack");
    let pack = pack::Pack::parse(&bytes).map_err(|e| format!("{path}: {}", e.0))?;
    describe(path, &pack)
}

fn describe(path: &str, pack: &pack::Pack<'_>) -> Result<Inspection, String> {
    if compile::game_language(&pack.manifest.game.language).is_none() {
        return Err(format!(
            "{path}: the game has no language {}",
            pack.manifest.game.language
        ));
    }
    let manifest = &pack.manifest;
    Ok(Inspection {
        path: path.to_owned(),
        feed: None,
        previous_key: pack
            .signature
            .as_ref()
            .and_then(|s| s.previous_key.as_ref())
            .map(|key| pack::fingerprint(key)),
        translation: Translation {
            hash: pack.hash_hex(),
            title: manifest.title.clone(),
            team: manifest.team.name.clone(),
            version: manifest.version.clone(),
            channel: manifest.channel.clone(),
            language: manifest.language.clone(),
            game_language: manifest.game.language.clone(),
            game_version: manifest.game.version.clone(),
            key: pack.signature.as_ref().map(pack::Signature::fingerprint),
            sheets: pack.sheet_count(),
            cells: pack.cell_count(),
            feed: None,
        },
    })
}

angelica_sdk::plugin!(Harmonia);
