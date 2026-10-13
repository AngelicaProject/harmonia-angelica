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
//! Harmonia in Dalamud comes first: when it translated the game in the
//! previous session on the same game build, this copy stays off for the
//! session (`library::begin_session`). It is the translation for the days
//! the game runs without Dalamud.
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
    /// The fingerprint of the key the feed says its packs are signed with.
    #[serde(default)]
    key: Option<String>,
    release: Option<feed::Release>,
    error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct DownloadInput {
    feed: String,
    release: feed::Release,
    /// The key fingerprint the feed declares, kept with the downloaded pack.
    #[serde(default)]
    key: Option<String>,
}

enum FeedStatus {
    UpToDate,
    Available(Box<feed::Release>),
    Problem(String),
}

struct FeedView {
    title: Option<String>,
    /// The key fingerprint the feed declares.
    key: Option<String>,
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
    /// A pack checked again: imported by hand, or downloaded from a feed.
    Inspect(Option<String>),
    Feeds,
    Download,
    Install,
    Compile(String),
}

// Independent flags of the window and the session, not a state machine.
#[allow(clippy::struct_excessive_bools)]
struct Harmonia {
    open: bool,
    library: Vec<Translation>,
    settings: Settings,
    /// The translation applied in this session.
    applied: Option<String>,
    /// Harmonia in Dalamud translates this session, so this copy does not.
    yielded: bool,
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
    /// The feed whose release downloads or installs now.
    downloading: Option<String>,
    /// The feeds the player asked to download: their release is used once
    /// it is installed.
    wanted: Vec<String>,
    /// Packs a previous game downloaded and did not install.
    resume: Vec<library::Download>,
    /// The pack the player chose to install, used once it is installed.
    chosen_pack: Option<String>,
    /// Whether the field for a link is shown.
    adding: bool,
    since_imports: f64,
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

/// `en` or `ru`, by the language of Angelica's interface now.
pub(crate) fn words(en: impl Into<String>, ru: impl Into<String>) -> String {
    if runtime::language() == "ru" {
        ru.into()
    } else {
        en.into()
    }
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
        let translation = self.translation(hash).cloned().ok_or_else(|| {
            words(
                "the chosen translation is no longer installed",
                "выбранный перевод больше не установлен",
            )
        })?;
        if !self.table_is_current(hash) {
            // The window asks for a restart and shows the preparation on the
            // translation's row; no message repeats it.
            self.compile(hash);
            return Ok(());
        }
        let info =
            excel_text::activate(TABLE, &library::table_path(hash), 0).map_err(|e| e.message)?;
        self.applied = Some(hash.to_owned());
        self.say(
            words(
                format!(
                    "{} {}: {} cells in {} sheets ready; the game translates them as it loads its text.",
                    translation.title, translation.version, info.cells, info.sheets
                ),
                format!(
                    "{} {}: готово ячеек: {}, листов: {}; игра переводит их, когда загружает текст.",
                    translation.title, translation.version, info.cells, info.sheets
                ),
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
                    words(
                        format!("Capital letters stay lowercase: {}", error.message),
                        format!("Заглавные буквы останутся строчными: {}", error.message),
                    ),
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
                words(
                    format!("The game's own fonts stay: {}", failed.join("; ")),
                    format!("Остаются шрифты игры: {}", failed.join("; ")),
                ),
                Tone::Warning,
            );
            0
        };
    }

    fn start_job(&mut self, name: &str, input: &impl Serialize, job: Job) {
        if self.job.is_some() {
            self.say(
                words(
                    "Wait for the running task to finish.",
                    "Дождитесь конца текущей задачи.",
                ),
                Tone::Warning,
            );
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
            Err(error) => self.say(
                words(
                    format!("Cannot start: {}", error.message),
                    format!("Не удалось начать: {}", error.message),
                ),
                Tone::Bad,
            ),
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
            self.say(
                words(
                    format!("Cannot save: {error}"),
                    format!("Не удалось сохранить: {error}"),
                ),
                Tone::Bad,
            );
        }
    }

    fn finished(&mut self, job: Job, output: &[u8]) -> Result<(), String> {
        match job {
            Job::Inspect(Some(feed)) => {
                let mut inspection: Inspection =
                    serde_json::from_slice(output).map_err(|e| e.to_string())?;
                inspection.feed = Some(feed);
                self.downloaded(inspection);
            }
            Job::Inspect(None) => {
                let inspection: Inspection =
                    serde_json::from_slice(output).map_err(|e| e.to_string())?;
                if self.translation(&inspection.translation.hash).is_some() {
                    self.say(
                        words(
                            "This pack is already installed.",
                            "Этот пак уже установлен.",
                        ),
                        Tone::Muted,
                    );
                } else {
                    self.inspection = Some(inspection);
                }
            }
            Job::Install => {
                let translation: Translation =
                    serde_json::from_slice(output).map_err(|e| e.to_string())?;
                self.say(
                    words(
                        format!("Installed {} {}.", translation.title, translation.version),
                        format!("Установлен {} {}.", translation.title, translation.version),
                    ),
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
                    words(
                        "Prepared again; the change applies at the next game start.",
                        "Подготовлено заново; изменение применится при следующем запуске игры.",
                    )
                } else {
                    words(
                        "Ready; it applies at the next game start.",
                        "Готово; применится при следующем запуске игры.",
                    )
                };
                let reserved = if stamp.summary.reserved > 0 {
                    words(
                        format!(
                            " {} cells hold reserved game tokens and are not written.",
                            stamp.summary.reserved
                        ),
                        format!(
                            " Не записаны ячейки с зарезервированными знаками игры: {}.",
                            stamp.summary.reserved
                        ),
                    )
                } else {
                    String::new()
                };
                self.say(
                    words(
                        format!(
                            "{ready} {} cells in {} sheets.{reserved}",
                            stamp.summary.cells, stamp.summary.sheets
                        ),
                        format!(
                            "{ready} Ячеек: {}, листов: {}.{reserved}",
                            stamp.summary.cells, stamp.summary.sheets
                        ),
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
        if let Some(url) = &translation.feed
            && let Some(at) = self.wanted.iter().position(|wanted| wanted == url)
        {
            self.wanted.remove(at);
            chosen = true;
        }
        if self.chosen_pack.as_deref() == Some(translation.hash.as_str()) {
            self.chosen_pack = None;
            chosen = true;
        }
        library::forget_download(&translation.hash);
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
            self.feeds.push((
                url.to_owned(),
                FeedView {
                    title,
                    key: None,
                    status,
                },
            ));
        }
    }

    /// Decides what a checked feed means for the installed translations. A
    /// newer release of a translation the feed already installed downloads
    /// by itself; a feed's first release waits for the player.
    fn evaluate(&mut self, checked: Checked) {
        let Checked {
            url,
            title,
            key,
            release,
            error,
        } = checked;
        if !self.settings.feeds.contains(&url) {
            return;
        }
        self.set_feed(&url, None, FeedStatus::UpToDate);
        if let Some((_, view)) = self.feeds.iter_mut().find(|(feed, _)| *feed == url) {
            view.key = key;
        }
        let Some(release) = release else {
            let reason = error.unwrap_or_else(|| {
                words(
                    "The feed has no stable release.",
                    "В ленте нет стабильного выпуска.",
                )
            });
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
            FeedStatus::Problem(words(
                format!(
                    "The release is made for the game language {}, which the game does not have.",
                    release.game_language
                ),
                format!(
                    "Выпуск сделан для языка игры {}, которого в игре нет.",
                    release.game_language
                ),
            ))
        } else if !release.is_newer_than(installed) {
            FeedStatus::UpToDate
        } else if !release.supported_by(env!("CARGO_PKG_VERSION")) {
            FeedStatus::Problem(words(
                "The newest release needs a newer Harmonia.",
                "Новому выпуску нужна более новая Harmonia.",
            ))
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
            let key = self
                .feeds
                .iter()
                .find(|(feed, _)| feed == url)
                .and_then(|(_, view)| view.key.clone());
            self.pending.push(DownloadInput {
                feed: url.to_owned(),
                release: release.clone(),
                key,
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
            return self.say(
                words(
                    "This pack is already installed.",
                    "Этот пак уже установлен.",
                ),
                Tone::Muted,
            );
        }
        let declared = inspection.feed.as_deref().and_then(|url| {
            self.feeds
                .iter()
                .find(|(feed, _)| feed == url)
                .and_then(|(_, view)| view.key.clone())
        });
        // The player chose this publisher when adding the link; a pack signed
        // with the key its feed declares, or with the key of the translation
        // the feed installed before, needs no second decision.
        let trusted = translation.key.as_ref().is_some_and(|key| {
            declared
                .as_ref()
                .is_some_and(|declared| declared.eq_ignore_ascii_case(key))
                || self
                    .library
                    .iter()
                    .any(|old| old.feed == inspection.feed && old.key.as_ref() == Some(key))
        });
        if trusted {
            let input = InstallInput {
                path: inspection.path.clone(),
                hash: translation.hash.clone(),
                feed: inspection.feed.clone(),
            };
            self.downloading.clone_from(&inspection.feed);
            self.start_job("install", &input, Job::Install);
        } else {
            self.say(
                words(
                    format!(
                        "{} {} is downloaded and waits for your decision in Harmonia.",
                        translation.title, translation.version
                    ),
                    format!(
                        "{} {} скачан и ждёт вашего решения в Harmonia.",
                        translation.title, translation.version
                    ),
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
                words(
                    "This does not look like a link. It must start with https://",
                    "Это не похоже на ссылку. Она должна начинаться с https://",
                ),
                Tone::Warning,
            );
        }
        if self.settings.feeds.contains(&url) {
            return self.say(
                words("This link is already added.", "Эта ссылка уже добавлена."),
                Tone::Muted,
            );
        }
        self.settings.feeds.push(url);
        self.feed_input.clear();
        self.save();
        self.next_check = 0.0;
    }

    /// Drops what waits for the player; a downloaded pack is removed.
    fn cancel_inspection(&mut self) {
        if let Some(inspection) = self.inspection.take()
            && let Some(feed) = &inspection.feed
        {
            let _ = storage::remove(&inspection.path);
            library::forget_download(&inspection.translation.hash);
            self.wanted.retain(|wanted| wanted != feed);
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
                if !self.wanted.contains(&url) {
                    self.wanted.push(url.clone());
                }
                self.request(&url, &release);
            }
        } else {
            match id {
                "feed-add" => {
                    self.add_feed();
                    if self.feed_input.is_empty() {
                        self.adding = false;
                    }
                }
                "feed-open" => self.adding = true,
                "feed-close" => {
                    self.adding = false;
                    self.feed_input.clear();
                }
                "feed-check" => self.next_check = 0.0,
                _ => return false,
            }
        }
        true
    }

    /// Installs the pack that waited for the player's decision.
    fn install_inspected(&mut self) {
        if self.job.is_some() {
            self.say(
                words(
                    "Wait for the running task to finish.",
                    "Дождитесь конца текущей задачи.",
                ),
                Tone::Warning,
            );
        } else if let Some(inspection) = self.inspection.take() {
            self.chosen_pack = Some(inspection.translation.hash.clone());
            self.downloading.clone_from(&inspection.feed);
            if let Some(feed) = &inspection.feed
                && !self.wanted.contains(feed)
            {
                self.wanted.push(feed.clone());
            }
            let input = InstallInput {
                path: inspection.path,
                hash: inspection.translation.hash,
                feed: inspection.feed,
            };
            self.start_job("install", &input, Job::Install);
        }
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
                self.start_job("inspect", &path, Job::Inspect(None));
            }
        } else if let Some(index) = id
            .strip_prefix("use-")
            .and_then(|i| i.parse::<usize>().ok())
        {
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
                self.say(
                    words(
                        format!("Removed {}.", removed.title),
                        format!("Удалён {}.", removed.title),
                    ),
                    Tone::Normal,
                );
            }
        } else {
            match id {
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
                "refresh" => self.imports = library::imports(),
                "install" => self.install_inspected(),
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
                .map(|t| format!("{} · {}", language_name(&t.language, self.russian), t.title))
        };
        if self.yielded {
            return Some((
                self.tr(
                    "Harmonia in Dalamud translates",
                    "Переводит Harmonia в Dalamud",
                )
                .to_owned(),
                Tone::Muted,
            ));
        }
        if self.applied != self.settings.active {
            let text = match name(&self.settings.active) {
                Some(chosen) => words(
                    format!("{chosen} — after a restart"),
                    format!("{chosen} — после перезапуска"),
                ),
                None => words(
                    "No translation after a restart",
                    "Без перевода после перезапуска",
                ),
            };
            return Some((text, Tone::Warning));
        }
        name(&self.applied).map(|applied| (applied, Tone::Good))
    }

    /// The one line that asks for a restart, while the game runs with
    /// another translation than the chosen one.
    fn restart_line(&self) -> Option<String> {
        if self.yielded || self.applied == self.settings.active {
            return None;
        }
        Some(
            match self
                .settings
                .active
                .as_deref()
                .and_then(|h| self.translation(h))
            {
                Some(t) => words(
                    format!("Restart the game to turn on {}.", t.title),
                    format!("Перезапустите игру, чтобы включить {}.", t.title),
                ),
                None => words(
                    "Restart the game to turn the translation off.",
                    "Перезапустите игру, чтобы выключить перевод.",
                ),
            },
        )
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

    /// An installed translation as a row: its version, where it stands, and
    /// under the pointer Use; the chosen one shows what it is.
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
            (false, false) => ("", Mark::None),
        };
        let chosen = self.chosen.as_deref() == Some(id.as_str());
        let mut entry = Entry::new(&id, &translation.title)
            .note(&translation.version)
            .value(state)
            .mark(mark)
            .chosen(chosen);
        if matches!(&self.job, Some((_, Job::Compile(compiling))) if compiling == hash) {
            entry = entry
                .progress(self.progress.0)
                .value(self.tr("preparing", "подготовка"));
        }
        if !active && idle {
            entry = entry.action(format!("use-{index}"), self.tr("Use", "Использовать"));
        }
        if !chosen {
            return entry;
        }
        let feed = translation.feed.as_deref();
        entry = entry
            .fact(self.tr("Team", "Команда"), &translation.team)
            .fact(
                self.tr("Translated", "Переведено"),
                format!("{} {}", translation.cells, self.tr("cells", "ячеек")),
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
                    || self.tr("from a file", "из файла").to_owned(),
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
        if active {
            entry = entry.more(
                "none",
                self.tr("Stop using", "Не использовать"),
                ActionKind::Normal,
                idle,
            );
        }
        if let Some(position) = translation
            .feed
            .as_deref()
            .and_then(|url| self.settings.feeds.iter().position(|f| f == url))
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
        let chosen = self.chosen.as_deref() == Some(id.as_str());
        let mut entry = Entry::new(&id, title).chosen(chosen);
        if self.downloading.as_deref() == Some(url) {
            entry = entry.progress(self.progress.0).value(&self.progress.1);
        } else {
            match view.map(|view| &view.status) {
                Some(FeedStatus::Available(release)) => {
                    entry = entry.note(&release.version).value(megabytes(release.size));
                    let waits = self.pending.iter().any(|pending| pending.feed == url);
                    if !waits && self.job.is_none() {
                        entry = entry
                            .action(format!("feed-get-{index}"), self.tr("Download", "Скачать"));
                    }
                }
                Some(FeedStatus::Problem(reason)) => {
                    entry = entry.note(reason).mark(Mark::Attention);
                }
                _ => entry = entry.note(self.tr("checking", "проверяется")),
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

    /// Every translation by its language, then the game without one, then
    /// files in the import folder.
    fn rows(&self, mut window: Window) -> Window {
        enum Row<'a> {
            Installed(usize, &'a Translation),
            Feed(usize, &'a str),
        }
        let mut languages: Vec<(String, Vec<Row<'_>>)> = Vec::new();
        let mut put = |language: String, row| match languages
            .iter_mut()
            .find(|(known, _)| *known == language)
        {
            Some((_, rows)) => rows.push(row),
            None => languages.push((language, vec![row])),
        };
        for (index, translation) in self.library.iter().enumerate() {
            put(
                translation.language.clone(),
                Row::Installed(index, translation),
            );
        }
        for (index, url) in self.settings.feeds.iter().enumerate() {
            if self
                .library
                .iter()
                .any(|t| t.feed.as_deref() == Some(url.as_str()))
            {
                continue;
            }
            let language = self
                .feeds
                .iter()
                .find(|(feed, _)| feed == url)
                .and_then(|(_, view)| match &view.status {
                    FeedStatus::Available(release) => Some(release.language.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            put(language, Row::Feed(index, url));
        }
        for (language, rows) in &languages {
            window = window.heading(if language.is_empty() {
                self.tr("By link", "По ссылке").to_owned()
            } else {
                language_name(language, self.russian)
            });
            for row in rows {
                window = window.entry(match row {
                    Row::Installed(index, translation) => {
                        self.translation_entry(*index, translation)
                    }
                    Row::Feed(index, url) => self.feed_entry(*index, url),
                });
            }
        }
        let original = self.library.first().map_or_else(
            || self.tr("The game's language", "Язык игры").to_owned(),
            |t| language_name(&t.game_language, self.russian),
        );
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
            .value(state)
            .mark(mark);
        if !active && self.job.is_none() {
            none = none.action("use-none", self.tr("Use", "Использовать"));
        }
        window = window.heading(original).entry(none);
        if !self.imports.is_empty() {
            window = window.heading(self.tr("Files", "Файлы"));
        }
        for (index, (path, size)) in self.imports.iter().enumerate() {
            let name = path.rsplit('/').next().unwrap_or(path);
            let mut entry = Entry::new(format!("row-i{index}"), name).value(megabytes(*size));
            if self.job.is_none() {
                entry = entry.action(format!("check-{index}"), self.tr("Check", "Проверить"));
            }
            window = window.entry(entry);
        }
        window
    }

    /// The field for a link, or the word that opens it.
    fn link(&self, window: Window) -> Window {
        if !self.adding {
            return window.row(|row| {
                row.action(
                    "feed-open",
                    self.tr("+ Add by link", "+ Добавить по ссылке"),
                    ActionKind::Normal,
                    true,
                )
            });
        }
        window
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
}

/// A language's name in the interface's language; the code when unknown.
fn language_name(code: &str, russian: bool) -> String {
    let (en, ru) = match code {
        "ru" => ("Russian", "Русский"),
        "en" => ("English", "Английский"),
        "ja" => ("Japanese", "Японский"),
        "de" => ("German", "Немецкий"),
        "fr" => ("French", "Французский"),
        "uk" => ("Ukrainian", "Украинский"),
        "zh" => ("Chinese", "Китайский"),
        "ko" => ("Korean", "Корейский"),
        _ => return code.to_owned(),
    };
    if russian { ru } else { en }.to_owned()
}

impl Plugin for Harmonia {
    fn start() -> Result<Self, String> {
        let mut plugin = Self {
            open: false,
            library: library::load_library()?,
            settings: library::load_settings()?,
            applied: None,
            yielded: false,
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
            downloading: None,
            wanted: Vec::new(),
            resume: library::downloaded(),
            chosen_pack: None,
            adding: false,
            since_imports: 0.0,
            russian: runtime::language() == "ru",
        };
        let _ = storage::remove(feed::DOWNLOAD);
        if plugin.started_with_game {
            // Without the count this copy cannot tell, and translates.
            plugin.yielded = match library::begin_session(&runtime::game_version()) {
                Ok(yielded) => yielded,
                Err(error) => {
                    angelica_sdk::warning(&format!("The session is not counted: {error}"));
                    false
                }
            };
            if plugin.yielded {
                angelica_sdk::info(
                    "Harmonia in Dalamud translated the previous session on this game build; this copy stays off.",
                );
            }
            if !plugin.yielded
                && let Some(hash) = plugin.settings.active.clone()
                && let Err(reason) = plugin.apply(&hash)
            {
                plugin.say(
                    words(
                        format!("No translation in this session: {reason}"),
                        format!("В этой игре без перевода: {reason}"),
                    ),
                    Tone::Warning,
                );
            }
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
                    self.downloading = None;
                    if let Err(error) = self.finished(job, &output) {
                        self.say(error, Tone::Bad);
                    }
                }
                jobs::State::Failed(reason) => {
                    self.job = None;
                    self.downloading = None;
                    self.say(reason, Tone::Bad);
                }
            }
        }
        self.since_check += elapsed;
        if self.job.is_none() {
            if let Some(download) = self.resume.pop() {
                let library::Download { path, feed, key } = download;
                if !self.wanted.contains(&feed) {
                    self.wanted.push(feed.clone());
                }
                // The feed is checked later; its key came with the pack.
                if self.feeds.iter().all(|(known, _)| *known != feed) {
                    self.set_feed(&feed, None, FeedStatus::UpToDate);
                }
                if let Some((_, view)) = self.feeds.iter_mut().find(|(known, _)| *known == feed)
                    && view.key.is_none()
                {
                    view.key = key;
                }
                self.downloading = Some(feed.clone());
                self.start_job("inspect", &path, Job::Inspect(Some(feed)));
            } else if let Some(input) = self.pending.pop() {
                self.downloading = Some(input.feed.clone());
                self.start_job("download", &input, Job::Download);
            } else if !self.settings.feeds.is_empty() && self.since_check >= self.next_check {
                self.since_check = 0.0;
                self.next_check = CHECK_EVERY;
                let feeds = self.settings.feeds.clone();
                self.start_job("feeds", &feeds, Job::Feeds);
            }
        }
        self.since_imports += elapsed;
        if self.open && self.since_imports >= 3.0 {
            self.since_imports = 0.0;
            self.imports = library::imports();
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
        if let Some(restart) = self.restart_line() {
            window = window.toned(restart, Tone::Warning);
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
        if self.yielded {
            window = window.muted(self.tr(
                "Harmonia in Dalamud translated the game in the previous session, so this copy stays off in this one. It translates from the next start once Harmonia in Dalamud does not run, for example while Dalamud waits for an update after a game update.",
                "В прошлой игре перевод делала Harmonia в Dalamud, поэтому в этой игре эта копия выключена. Она начнёт переводить со следующего запуска, если Harmonia в Dalamud не запустится, например пока Dalamud ждёт обновления после обновления игры.",
            ));
        }
        window = self.rows(window);
        if matches!(&self.job, Some((_, job)) if !matches!(job, Job::Download | Job::Compile(_))) {
            window = window.progress(self.progress.0, &self.progress.1);
        }
        if !self.message.0.is_empty() {
            window = window.toned(&self.message.0, self.message.1);
        }
        window = self.link(window.separator());
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
                    return Err(words(
                        "the file changed after it was checked; check it again",
                        "файл изменился после проверки; проверьте его ещё раз",
                    ));
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
                let _ = jobs::report(0.4, &words("Checking the pack", "Проверка пака"));
                let pack = pack::Pack::parse(&bytes).map_err(|e| e.0)?;
                if pack.hash_hex() != input.hash {
                    return Err(words(
                        "the installed pack changed; install it again",
                        "установленный пак изменился; установите его ещё раз",
                    ));
                }
                let _ = jobs::report(
                    0.6,
                    &words("Preparing the translation", "Подготовка перевода"),
                );
                let (table, summary) = compile::compile(&pack, &input.kept_sheets)?;
                let _ = jobs::report(0.8, &words("Preparing fonts", "Подготовка шрифтов"));
                let (fonts, font_reports) = prepare_fonts(&pack, &input.hash)?;
                let _ = jobs::report(0.9, &words("Saving", "Сохранение"));
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
        .map_err(|error| {
            words(
                format!("The feed could not be read: {}", error.message),
                format!("Не удалось прочитать ленту: {}", error.message),
            )
        })
        .and_then(|response| match response.head.status {
            200..=299 => feed::parse(&response.body).map_err(|reason| {
                let mut reason = reason;
                if let Some(first) = reason.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                format!("{reason}.")
            }),
            404 | 410 => Err(words(
                "Nothing is published at this link.",
                "По этой ссылке ничего не опубликовано.",
            )),
            status => Err(words(
                format!("The server answered {status}."),
                format!("Сервер ответил {status}."),
            )),
        });
    match feed {
        Ok(feed) => Checked {
            url,
            release: feed.newest().cloned(),
            title: feed.title,
            key: feed.key,
            error: None,
        },
        Err(error) => Checked {
            url,
            title: None,
            key: None,
            release: None,
            error: Some(error),
        },
    }
}

/// Downloads a release, verifies the file and the pack in it against the
/// feed, and leaves the pack in storage for the install job.
fn download(input: &DownloadInput) -> Result<Inspection, String> {
    let release = &input.release;
    let _ = jobs::report(0.0, &words("Downloading", "Скачивание"));
    let (head, _) = http::download(&release.url, feed::DOWNLOAD).map_err(|e| e.message)?;
    if !head.is_success() {
        return Err(words(
            format!("the download failed: the server answered {}", head.status),
            format!("скачать не удалось: сервер ответил {}", head.status),
        ));
    }
    let file = library::read_file(feed::DOWNLOAD, release.size);
    let _ = storage::remove(feed::DOWNLOAD);
    let _ = jobs::report(0.4, &words("Unpacking", "Распаковка"));
    let bytes = feed::unpack(release, file?)?;
    let _ = jobs::report(0.6, &words("Checking the pack", "Проверка пака"));
    let path = format!("downloads/{}.hpk", release.pack_hash);
    let pack = pack::Pack::parse(&bytes).map_err(|e| e.0)?;
    feed::check_pack(release, &pack)?;
    let mut inspection = describe(&path, &pack)?;
    inspection.feed = Some(input.feed.clone());
    let _ = jobs::report(0.9, &words("Saving", "Сохранение"));
    storage::write(&path, &bytes).map_err(|e| e.message)?;
    library::remember_download(&release.pack_hash, &input.feed, input.key.as_deref())?;
    Ok(inspection)
}

/// Verifies a pack completely and describes it.
fn inspect(path: &str) -> Result<Inspection, String> {
    let bytes = library::read_file(path, pack::MAX_PACK)?;
    let _ = jobs::report(0.4, &words("Checking the pack", "Проверка пака"));
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
