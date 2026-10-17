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
//! the same key install by themselves unless the player turned that off.
//!
//! The window has four tabs: the translation (`tab_translation`), the
//! name dictionary (`tab_dictionary`), what stays in the game's language
//! (`tab_coverage`), and settings (`tab_settings`).
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
pub mod catalog;
pub mod compile;
pub mod coverage;
pub mod fdt;
pub mod feed;
pub mod font_patch;
pub mod fonts;
pub mod library;
pub mod names;
pub mod pack;
mod tab_coverage;
mod tab_dictionary;
mod tab_settings;
mod tab_translation;

use angelica_sdk::{
    Plugin,
    api::{file_replacements, game_files, jobs, runtime, storage},
    chat, commands, http, lens,
    ui::{Event, Tone, Window},
};
use library::{Settings, Stamp, Translation};
use serde::{Deserialize, Serialize};

/// The map's event of a text cell of a row the game builds, whose rules
/// the translation's table holds.
const SHEET_TEXT: &str = "sheet_text";
const WINDOW: &str = "main";

/// The translation Harmonia suggests while none is installed, and the key
/// its publisher signs with: a release signed with it needs no question.
pub const SUGGESTED_FEED: &str =
    "https://angelicaproject.github.io/ProjectPrima/harmonia/feed-v1.json";
const SUGGESTED_KEY: &str = "836b79b354f1aa84faa669fc49a0c62f23f3ade5c4d2a3d40f6d03e02afc40c7";

/// Seconds after the start before the first check of the feeds.
const FIRST_CHECK: f64 = 30.0;
/// Seconds after the last change of the coverage before the table is
/// prepared again.
const SETTLE: f64 = 1.5;
/// Names a search shows.
const SHOWN_NAMES: usize = 60;

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
#[derive(Clone, Serialize, Deserialize)]
struct Checked {
    url: String,
    title: Option<String>,
    /// The fingerprint of the key the feed says its packs are signed with.
    #[serde(default)]
    key: Option<String>,
    release: Option<feed::Release>,
    error: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct FeedsInput {
    feeds: Vec<String>,
    testing: bool,
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
    /// The newest release the feed offers, installed or not.
    newest: Option<feed::Release>,
}

#[derive(Serialize, Deserialize)]
struct CompileInput {
    hash: String,
    coverage: coverage::Coverage,
}

#[derive(Serialize, Deserialize)]
struct SearchInput {
    query: String,
    category: Option<names::Category>,
    limit: usize,
}

/// The work on packs: one at a time.
enum Job {
    /// A pack checked again: imported by hand, or downloaded from a feed.
    Inspect(Option<String>),
    Feeds,
    Download,
    Install,
    Compile(String),
}

/// Work beside it, each kind at most once.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Side {
    /// The catalog of a translation's sheets.
    Catalog(String),
    /// The name index, with its header.
    Names(String),
    /// A search in the name index; `chat` for one `/hfind` asked for.
    Search { chat: bool },
    /// The suggested translation's feed.
    Suggestion,
}

impl Side {
    fn same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// The window's tabs, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Translation,
    Dictionary,
    Coverage,
    Settings,
}

impl Tab {
    const ALL: [Self; 4] = [
        Self::Translation,
        Self::Dictionary,
        Self::Coverage,
        Self::Settings,
    ];

    fn label(self, russian: bool) -> &'static str {
        match (self, russian) {
            (Self::Translation, false) => "Translation",
            (Self::Translation, true) => "Перевод",
            (Self::Dictionary, false) => "Dictionary",
            (Self::Dictionary, true) => "Словарь",
            (Self::Coverage, false) => "Coverage",
            (Self::Coverage, true) => "Охват",
            (Self::Settings, false) => "Settings",
            (Self::Settings, true) => "Настройки",
        }
    }
}

/// The Coverage tab's state.
#[derive(Default)]
struct CoverageView {
    /// The chosen group.
    group: Option<String>,
    /// The entry of the group whose columns are shown.
    entry: Option<String>,
    /// The search for sheets to add, while it is open.
    adding: Option<String>,
    /// The new name, while the group is renamed.
    renaming: Option<String>,
    /// The group the player asked to delete, until they confirm.
    deleting: Option<String>,
    catalog: Option<catalog::Catalog>,
    /// The catalog while it is read from its file.
    loader: Option<catalog::Loader>,
    /// The sheets the search for sheets to add found: the query, the
    /// first ones, and how many there are.
    found: Option<(String, Vec<String>, usize)>,
    /// Why the catalog could not be made.
    failed: Option<String>,
}

/// The Dictionary tab's state.
#[derive(Default)]
struct DictionaryView {
    query: String,
    filter: Option<names::Category>,
    found: Option<names::Found>,
    /// The query changed while a search ran.
    again: bool,
    /// The header of the index in storage, once read.
    header: Option<String>,
    built: Option<names::Built>,
    failed: Option<String>,
    chosen: Option<(names::Category, u32)>,
    copied: Option<(names::Category, u32)>,
    /// What `/hfind` asked for, until the index is ready.
    chat: Option<String>,
}

// Independent flags of the window and the session, not a state machine.
#[allow(clippy::struct_excessive_bools)]
struct Harmonia {
    open: bool,
    tab: Tab,
    library: Vec<Translation>,
    settings: Settings,
    /// The translation applied in this session.
    applied: Option<String>,
    /// The coverage of the table applied in this session.
    applied_coverage: Option<String>,
    /// What the applied table holds.
    summary: Option<compile::Summary>,
    /// Harmonia in Dalamud translates this session, so this copy does not.
    yielded: bool,
    started_with_game: bool,
    imports: Vec<(String, u64)>,
    inspection: Option<Inspection>,
    job: Option<(u64, Job)>,
    progress: (f32, String),
    side: Vec<(u64, Side, f32)>,
    message: (String, Tone),
    status: Option<lens::DecisionStatus>,
    since_status: f64,
    /// The applied translation's table, as rules for the game's sheet text.
    table: Option<lens::RuleFile>,
    /// The rules that give the game's case conversion Cyrillic letters.
    letters: Vec<lens::Rule>,
    /// Font files served in this session.
    fonts: usize,
    /// What the last check found, by feed link.
    feeds: Vec<(String, FeedView)>,
    feed_input: String,
    since_check: f64,
    next_check: f64,
    /// Seconds since the start; when the feeds were last checked.
    clock: f64,
    checked_at: Option<f64>,
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
    /// The link the player just added, until its feed is read.
    added: Option<String>,
    /// The feed whose publisher the player is asked to trust.
    asking: Option<String>,
    /// The translation the player asked to remove, by its place in the
    /// library, until they confirm.
    removing: Option<usize>,
    /// Whether the field for a link is shown.
    adding: bool,
    /// The release of the suggested translation, once its feed is read.
    suggestion: Option<Checked>,
    /// The translation whose «What's new» is open.
    news: Option<String>,
    /// Seconds until the table is prepared again after a coverage change.
    settle: Option<f64>,
    since_imports: f64,
    coverage: CoverageView,
    dictionary: DictionaryView,
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

/// A count with its thousands apart: `412 806`, or `412,806` in English.
fn count(number: impl Into<u64>, russian: bool) -> String {
    let digits = number.into().to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(if russian { ' ' } else { ',' });
        }
        out.push(digit);
    }
    out
}

/// `en` or `ru`, by the language of Angelica's interface now.
pub(crate) fn words(en: impl Into<String>, ru: impl Into<String>) -> String {
    if runtime::language() == "ru" {
        ru.into()
    } else {
        en.into()
    }
}

/// A language's name in the interface's language; the code when unknown.
fn language_name(code: &str, russian: bool) -> String {
    let (en, ru) = match code.split('-').next().unwrap_or(code) {
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

    /// `en` or `ru`, by the language of Angelica's interface.
    fn tr<'a>(&self, en: &'a str, ru: &'a str) -> &'a str {
        if self.russian { ru } else { en }
    }

    fn translation(&self, hash: &str) -> Option<&Translation> {
        self.library.iter().find(|t| t.hash == hash)
    }

    /// The game's language number: the translation's, else English.
    fn game_language(&self) -> u8 {
        self.applied
            .as_deref()
            .or(self.settings.active.as_deref())
            .and_then(|hash| self.translation(hash))
            .or_else(|| self.library.first())
            .and_then(|t| compile::game_language(&t.game_language))
            .unwrap_or(angelica_sdk::excel::ENGLISH)
    }

    /// Whether the table of `hash` was built with the current coverage and
    /// its fonts from the running game's files.
    fn table_is_current(&self, hash: &str) -> bool {
        matches!(
            library::load_stamp(hash),
            Ok(Some(stamp)) if stamp.coverage == self.settings.coverage.fingerprint()
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
        let table =
            lens::RuleFile::new(SHEET_TEXT, &library::table_path(hash)).map_err(|e| e.message)?;
        let (cells, sheets) = (table.info().rules, table.info().parts);
        self.table = Some(table);
        self.applied = Some(hash.to_owned());
        self.applied_coverage = Some(self.settings.coverage.fingerprint());
        angelica_sdk::info(&format!(
            "{} {}: {cells} cells in {sheets} sheets ready; the game translates them as it loads its text.",
            translation.title, translation.version
        ));
        if let Ok(Some(stamp)) = library::load_stamp(hash) {
            self.summary = Some(stamp.summary);
            self.replace_fonts(&stamp);
        }
        if compile::is_cyrillic(&translation.language)
            && let Err(error) = self.add_letters()
        {
            self.say(
                words(
                    format!("Capital letters stay lowercase: {}", error.message),
                    format!("Заглавные буквы останутся строчными: {}", error.message),
                ),
                Tone::Warning,
            );
        }
        Ok(())
    }

    /// Gives the game's case conversion the Cyrillic letters, both ways.
    fn add_letters(&mut self) -> Result<(), angelica_sdk::Error> {
        let mut rules = Vec::new();
        for (small, capital) in compile::cyrillic_letters() {
            rules.push(lens::Rule::with(
                &format!("letter_upper[letter = \"{small}\"]"),
                "becomes",
                capital.to_string(),
            )?);
            rules.push(lens::Rule::with(
                &format!("letter_lower[letter = \"{capital}\"]"),
                "becomes",
                small.to_string(),
            )?);
        }
        self.letters = rules;
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

    /// Starts work beside the packs' unless the same kind runs.
    fn start_side(&mut self, name: &str, input: &impl Serialize, side: Side) -> bool {
        if self
            .side
            .iter()
            .any(|(_, running, _)| running.same_kind(&side))
        {
            return false;
        }
        let Ok(input) = serde_json::to_vec(input) else {
            return false;
        };
        match jobs::start(name, &input) {
            Ok(id) => {
                self.side.push((id, side, 0.0));
                true
            }
            Err(error) => {
                angelica_sdk::warning(&format!("{name} cannot start: {}", error.message));
                false
            }
        }
    }

    fn side_running(&self, kind: &Side) -> Option<f32> {
        self.side
            .iter()
            .find(|(_, side, _)| side.same_kind(kind))
            .map(|(_, _, fraction)| *fraction)
    }

    fn compile(&mut self, hash: &str) {
        let input = CompileInput {
            hash: hash.to_owned(),
            coverage: self.settings.coverage.clone(),
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

    /// The coverage changed: saved now, the table prepared again once the
    /// player stops changing it.
    fn coverage_changed(&mut self) {
        self.save();
        self.settle = Some(SETTLE);
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
                self.checked_at = Some(self.clock);
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
                            "Ready; it applies at the next game start. {} cells in {} sheets.{reserved}",
                            stamp.summary.cells, stamp.summary.sheets
                        ),
                        format!(
                            "Готово; применится при следующем запуске игры. Ячеек: {}, листов: {}.{reserved}",
                            stamp.summary.cells, stamp.summary.sheets
                        ),
                    ),
                    Tone::Good,
                );
                if self
                    .coverage
                    .catalog
                    .as_ref()
                    .is_none_or(|c| c.hash != hash)
                {
                    self.coverage.catalog = None;
                }
            }
        }
        Ok(())
    }

    fn side_finished(&mut self, side: Side, output: Result<Vec<u8>, String>) {
        match side {
            Side::Catalog(_) => match output.and_then(|bytes| {
                catalog::Loader::new(bytes).ok_or_else(|| "not a sheet list".into())
            }) {
                Ok(loader) => {
                    self.coverage.loader = Some(loader);
                    self.coverage.failed = None;
                }
                Err(error) => {
                    angelica_sdk::warning(&format!("The sheet list could not be made: {error}"));
                    self.coverage.failed = Some(error);
                }
            },
            Side::Names(header) => match output.and_then(|bytes| {
                serde_json::from_slice::<names::Built>(&bytes).map_err(|e| e.to_string())
            }) {
                Ok(built) => {
                    angelica_sdk::info(&format!(
                        "The name dictionary: {} names; without names in the game map: {:?}. {}",
                        built.names,
                        built.missing,
                        built.notes.join("; ")
                    ));
                    self.dictionary.header = Some(header);
                    self.dictionary.built = Some(built);
                    self.dictionary.failed = None;
                    if let Some(query) = self.dictionary.chat.clone() {
                        self.search(&query, None, true);
                    }
                    self.dictionary.again = true;
                }
                Err(error) => {
                    angelica_sdk::warning(&format!(
                        "The name dictionary could not be built: {error}"
                    ));
                    self.dictionary.failed = Some(error);
                    self.dictionary.chat = None;
                }
            },
            Side::Search { chat } => {
                let found = output.and_then(|bytes| {
                    serde_json::from_slice::<names::Found>(&bytes).map_err(|e| e.to_string())
                });
                match (found, chat) {
                    (Ok(found), true) => self.print_found(&found),
                    (Ok(found), false) => self.dictionary.found = Some(found),
                    (Err(error), _) => {
                        angelica_sdk::warning(&format!("The name search failed: {error}"));
                    }
                }
            }
            Side::Suggestion => {
                if let Ok(mut checked) = output.and_then(|bytes| {
                    serde_json::from_slice::<Vec<Checked>>(&bytes).map_err(|e| e.to_string())
                }) {
                    self.suggestion = checked.pop();
                }
            }
        }
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
                    newest: None,
                },
            ));
        }
    }

    /// Decides what a checked feed means for the installed translations. A
    /// newer release of a translation the feed already installed downloads
    /// by itself when the player lets updates install; a feed's first
    /// release waits for the player.
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
            view.key.clone_from(&key);
            view.newest.clone_from(&release);
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
            .map(|t| t.version.clone())
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
        } else if !release.is_newer_than(installed.as_deref()) {
            FeedStatus::UpToDate
        } else if !release.supported_by(env!("CARGO_PKG_VERSION")) {
            FeedStatus::Problem(words(
                "The newest release needs a newer Harmonia.",
                "Новому выпуску нужна более новая Harmonia.",
            ))
        } else {
            if installed.is_some() && self.settings.auto_install {
                self.request(&url, &release);
            }
            FeedStatus::Available(Box::new(release.clone()))
        };
        // A link the player just added: ask whether to trust its publisher
        // and download. The suggested translation's key is known.
        let first = self.added.as_deref() == Some(url.as_str())
            && matches!(status, FeedStatus::Available(_))
            && installed.is_none();
        self.set_feed(&url, title, status);
        if self.added.as_deref() == Some(url.as_str()) {
            self.added = None;
            let known = url == SUGGESTED_FEED
                && key
                    .as_deref()
                    .is_some_and(|key| key.eq_ignore_ascii_case(SUGGESTED_KEY));
            if first && known {
                if !self.wanted.contains(&url) {
                    self.wanted.push(url.clone());
                }
                self.request(&url, &release);
            } else if first {
                self.asking = Some(url);
            }
        }
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

    fn add_feed(&mut self, url: &str) {
        let url = url.trim().to_owned();
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
        self.settings.feeds.push(url.clone());
        self.added = Some(url);
        self.feed_input.clear();
        self.save();
        self.next_check = 0.0;
        self.since_check = 0.0;
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

    /// The player trusts the publisher of the feed they added: its release
    /// downloads, installs, and is used.
    fn trust(&mut self) {
        let Some(url) = self.asking.take() else {
            return;
        };
        let release = self
            .feeds
            .iter()
            .find(|(feed, _)| *feed == url)
            .and_then(|(_, view)| match &view.status {
                FeedStatus::Available(release) => Some((**release).clone()),
                _ => None,
            });
        if let Some(release) = release {
            if !self.wanted.contains(&url) {
                self.wanted.push(url.clone());
            }
            self.request(&url, &release);
        }
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

    /// The title bar's line: what waits for a game start, else what is
    /// applied.
    fn status_line(&self) -> (String, Tone) {
        let words = |en: &str, ru: &str| self.tr(en, ru).to_owned();
        if self.yielded {
            return (
                words(
                    "Harmonia in Dalamud translates",
                    "Переводит Harmonia в Dalamud",
                ),
                Tone::Muted,
            );
        }
        if self.restart_line().is_some() {
            return (
                words("Restart the game", "Перезапустите игру"),
                Tone::Warning,
            );
        }
        match self
            .applied
            .as_deref()
            .and_then(|hash| self.translation(hash))
        {
            Some(applied) => {
                let language = language_name(&applied.language, self.russian);
                (
                    if self.russian {
                        format!("{language} в игре")
                    } else {
                        format!("{language} in the game")
                    },
                    Tone::Good,
                )
            }
            None => (words("Translation off", "Перевод выключен"), Tone::Muted),
        }
    }

    /// The one line that asks for a restart, while the game runs with
    /// another translation or coverage than the chosen one.
    fn restart_line(&self) -> Option<String> {
        if self.yielded {
            return None;
        }
        if self.applied != self.settings.active {
            return Some(
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
            );
        }
        (self.applied.is_some()
            && self.applied_coverage.as_deref() != Some(&self.settings.coverage.fingerprint()))
        .then(|| {
            words(
                "Restart the game to apply what stays in the game's language.",
                "Перезапустите игру, чтобы применить, что остаётся на языке игры.",
            )
        })
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

    /// The build the name index must come from now.
    fn names_build(&self) -> names::Build {
        let language = self.game_language();
        let others = self
            .settings
            .dictionary_languages
            .iter()
            .filter_map(|tag| names::LANGUAGES.iter().find(|(known, _)| known == tag))
            .map(|(_, number)| *number)
            .filter(|number| *number != language)
            .collect();
        names::Build {
            hash: self.applied.clone(),
            coverage: self.settings.coverage.clone(),
            language,
            others,
            game_version: runtime::game_version(),
        }
    }

    /// Whether the index in storage is the current one; starts building it
    /// when it is not.
    fn names_ready(&mut self) -> bool {
        let build = self.names_build();
        let header = build.header();
        if self.dictionary.header.is_none() {
            self.dictionary.header =
                storage::read_range(names::INDEX, 0, 8192)
                    .ok()
                    .and_then(|bytes| {
                        String::from_utf8_lossy(&bytes)
                            .lines()
                            .next()
                            .map(str::to_owned)
                    });
        }
        if self.dictionary.header.as_deref() == Some(header.as_str()) {
            return true;
        }
        if self.dictionary.failed.is_none() {
            self.start_side("names", &build, Side::Names(header));
        }
        false
    }

    fn search(&mut self, query: &str, category: Option<names::Category>, chat: bool) {
        let input = SearchInput {
            query: query.to_owned(),
            category,
            limit: if chat { 5 } else { SHOWN_NAMES },
        };
        if chat {
            self.dictionary.chat = None;
        }
        if !self.start_side("search", &input, Side::Search { chat }) && !chat {
            self.dictionary.again = true;
        }
    }

    /// The window's search again, when its query changed.
    fn refresh_search(&mut self) {
        if !self.dictionary.again || self.side_running(&Side::Search { chat: false }).is_some() {
            return;
        }
        self.dictionary.again = false;
        let query = self.dictionary.query.trim().to_owned();
        if query.chars().count() < names::MIN_QUERY {
            self.dictionary.found = None;
            return;
        }
        if self.names_ready() {
            let filter = self.dictionary.filter;
            self.search(&query, filter, false);
        }
    }

    /// What `/hfind` found, in the chat log.
    fn print_found(&self, found: &names::Found) {
        let mut message = chat::Message::new();
        if found.matches.is_empty() {
            message = message.text(words(
                format!("No names match “{}”.", found.query),
                format!("По запросу «{}» ничего не найдено.", found.query),
            ));
        } else {
            message = message.text(words(
                format!("“{}”: {} found", found.query, found.total),
                format!("«{}»: найдено {}", found.query, found.total),
            ));
            for name in &found.matches {
                message = message.line().accent(&name.shown);
                if name.is_translated() {
                    message = message.muted(format!(" — {}", name.original()));
                }
                message = message.muted(format!(
                    " ({})",
                    tab_dictionary::category_word(name.category, self.russian)
                ));
            }
            if found.total > found.matches.len() {
                message = message.line().muted(words(
                    format!(
                        "…and {} more in the Dictionary tab of Harmonia.",
                        found.total - found.matches.len()
                    ),
                    format!(
                        "…и ещё {} во вкладке «Словарь» Harmonia.",
                        found.total - found.matches.len()
                    ),
                ));
            }
        }
        if let Err(error) = message.print() {
            angelica_sdk::warning(&format!("/hfind could not print: {}", error.message));
        }
    }

    /// The catalog of `hash`: loaded from storage when current, else made
    /// in a job.
    fn want_catalog(&mut self, hash: &str) {
        if self
            .coverage
            .catalog
            .as_ref()
            .is_some_and(|catalog| catalog.hash == hash)
            || self.coverage.failed.is_some()
        {
            return;
        }
        if self
            .coverage
            .loader
            .as_ref()
            .is_some_and(|loader| loader.hash() == hash)
            || self.side_running(&Side::Catalog(String::new())).is_some()
        {
            return;
        }
        let stored = storage::read(&catalog::path(hash))
            .ok()
            .and_then(catalog::Loader::new)
            .filter(|loader| loader.game_version() == runtime::game_version());
        if stored.is_some() {
            self.coverage.loader = stored;
        } else {
            self.start_side("catalog", &hash, Side::Catalog(hash.to_owned()));
        }
    }

    /// Reads a few more lines of the catalog being loaded.
    fn load_catalog(&mut self) {
        let Some(loader) = &mut self.coverage.loader else {
            return;
        };
        match loader.step(catalog::LINES_A_FRAME) {
            Ok(None) => {}
            Ok(Some(catalog)) => {
                angelica_sdk::info(&format!(
                    "The sheet list of the translation: {} sheets, {} quest names.",
                    catalog.sheets.len(),
                    catalog.quests.len()
                ));
                self.coverage.loader = None;
                self.coverage.found = None;
                self.coverage.catalog = Some(catalog);
            }
            Err(error) => {
                angelica_sdk::warning(&format!("The sheet list could not be read: {error}"));
                self.coverage.loader = None;
                self.coverage.failed = Some(error);
            }
        }
    }

    fn poll_side(&mut self) -> Result<(), String> {
        let mut index = 0;
        while index < self.side.len() {
            let id = self.side[index].0;
            match jobs::poll(id).map_err(|e| e.message)? {
                jobs::State::Running(progress) => {
                    self.side[index].2 = progress.fraction;
                    index += 1;
                }
                jobs::State::Finished(output) => {
                    let (_, side, _) = self.side.remove(index);
                    self.side_finished(side, Ok(output));
                }
                jobs::State::Failed(reason) => {
                    let (_, side, _) = self.side.remove(index);
                    self.side_finished(side, Err(reason));
                }
            }
        }
        Ok(())
    }

    fn window(&mut self) -> Window {
        let labels: Vec<&str> = Tab::ALL.iter().map(|tab| tab.label(self.russian)).collect();
        let selected = Tab::ALL
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0);
        let mut window = Window::new(WINDOW, "Harmonia");
        let (text, tone) = self.status_line();
        window = window.status(text, tone);
        let mut path = Vec::new();
        if self.tab != Tab::Translation {
            path.push(self.tab.label(self.russian).to_owned());
        }
        if self.tab == Tab::Coverage
            && let Some(group) = self.coverage_group()
        {
            path.push(group.title(self.russian));
        }
        window = window.path(path);
        #[allow(clippy::cast_possible_truncation)]
        let window = window.tabs("tab", &labels, selected as u32);
        let window = match self.tab {
            Tab::Translation => self.translation_tab(window),
            Tab::Dictionary => self.dictionary_tab(window),
            Tab::Coverage => self.coverage_tab(window),
            Tab::Settings => self.settings_tab(window),
        };
        match self.dialog() {
            Some(dialog) => window.dialog(dialog),
            None => window,
        }
    }

    fn click(&mut self, id: &str) {
        let handled = match self.tab {
            Tab::Translation => self.translation_click(id),
            Tab::Dictionary => self.dictionary_click(id),
            Tab::Coverage => self.coverage_click(id),
            Tab::Settings => false,
        };
        // A dialog's answer may come while another tab is shown.
        if !handled {
            self.translation_click(id);
        }
    }

    fn open_tab(&mut self, tab: Tab) {
        self.tab = tab;
        match tab {
            Tab::Dictionary => {
                self.names_ready();
            }
            Tab::Coverage => {
                if let Some(hash) = self.coverage_hash() {
                    self.want_catalog(&hash);
                }
            }
            _ => {}
        }
    }
}

impl Plugin for Harmonia {
    fn start() -> Result<Self, String> {
        let mut plugin = Self {
            open: false,
            tab: Tab::Translation,
            library: library::load_library()?,
            settings: library::load_settings()?,
            applied: None,
            applied_coverage: None,
            summary: None,
            yielded: false,
            started_with_game: runtime::started_with_game(),
            imports: library::imports(),
            inspection: None,
            job: None,
            progress: (0.0, String::new()),
            side: Vec::new(),
            message: (String::new(), Tone::Muted),
            status: None,
            since_status: f64::INFINITY,
            table: None,
            letters: Vec::new(),
            fonts: 0,
            feeds: Vec::new(),
            feed_input: String::new(),
            since_check: 0.0,
            next_check: FIRST_CHECK,
            clock: 0.0,
            checked_at: None,
            pending: Vec::new(),
            chosen: None,
            downloading: None,
            wanted: Vec::new(),
            resume: library::downloaded(),
            chosen_pack: None,
            added: None,
            asking: None,
            removing: None,
            adding: false,
            suggestion: None,
            news: None,
            settle: None,
            since_imports: 0.0,
            coverage: CoverageView::default(),
            dictionary: DictionaryView::default(),
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
        let find = commands::Command::new(
            "hfind",
            words(
                "Finds a name in the game's language and in the translation",
                "Ищет название на языке игры и в переводе",
            ),
        )
        .parameter(commands::Parameter::text(
            "name",
            words("The name, in any language", "Название на любом языке"),
        ));
        if let Err(error) = commands::define(&[find]) {
            angelica_sdk::warning(&format!("/hfind is not available: {}", error.message));
        }
        Ok(plugin)
    }

    fn tick(&mut self, elapsed: f64) -> Result<(), String> {
        self.clock += elapsed;
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
        self.poll_side()?;
        self.load_catalog();
        self.refresh_found();
        if let Some(left) = &mut self.settle {
            *left -= elapsed;
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
            } else if self.settle.is_some_and(|left| left <= 0.0) {
                self.settle = None;
                if let Some(hash) = self.settings.active.clone()
                    && !self.table_is_current(&hash)
                {
                    self.compile(&hash);
                }
            } else if !self.settings.feeds.is_empty() && self.since_check >= self.next_check {
                self.since_check = 0.0;
                self.next_check = f64::from(self.settings.check_minutes) * 60.0;
                let input = FeedsInput {
                    feeds: self.settings.feeds.clone(),
                    testing: self.settings.testing,
                };
                self.start_job("feeds", &input, Job::Feeds);
            }
        }
        self.refresh_search();
        self.since_imports += elapsed;
        if self.open && self.since_imports >= 3.0 {
            self.since_imports = 0.0;
            self.imports = library::imports();
        }
        self.since_status += elapsed;
        if self.applied.is_some() && self.since_status >= 1.0 {
            self.since_status = 0.0;
            self.status = self.table.as_ref().and_then(|table| table.decisions().ok());
        }
        Ok(())
    }

    fn ui(&mut self) -> Vec<Window> {
        if !self.open {
            return Vec::new();
        }
        self.russian = runtime::language() == "ru";
        vec![self.window()]
    }

    fn ui_event(&mut self, event: Event) {
        match event {
            Event::OpenRequested => {
                self.open = true;
                self.imports = library::imports();
                if self.library.is_empty() && self.suggestion.is_none() {
                    let input = FeedsInput {
                        feeds: vec![SUGGESTED_FEED.to_owned()],
                        testing: false,
                    };
                    self.start_side("feeds", &input, Side::Suggestion);
                }
            }
            // Closing a question is the same as not answering it.
            Event::Closed(place) if place == "trust" => {
                self.translation_click("distrust");
            }
            Event::Closed(place) if place == "install" => self.cancel_inspection(),
            Event::Closed(place) if place == "remove" => self.removing = None,
            Event::Closed(place) if place == "delete-group" => self.coverage.deleting = None,
            Event::Closed(_) => self.open = false,
            Event::Clicked(target) => self.click(&target.id),
            Event::Chosen(chosen) if chosen.id == "tab" => {
                if let Some(tab) = Tab::ALL.get(chosen.index as usize) {
                    self.open_tab(*tab);
                }
            }
            Event::Chosen(chosen) if chosen.id == "groups" => self.coverage_chosen(chosen.index),
            Event::Chosen(chosen) => self.settings_chosen(&chosen.id, chosen.index),
            Event::Navigated(navigated) => {
                if navigated.depth == 0 {
                    self.open_tab(Tab::Translation);
                } else if self.tab == Tab::Coverage {
                    self.coverage.entry = None;
                    self.coverage.adding = None;
                }
            }
            Event::Toggled(toggled) => {
                if self.tab == Tab::Coverage {
                    self.coverage_toggled(&toggled.id, toggled.checked);
                } else {
                    self.settings_toggled(&toggled.id, toggled.checked);
                }
            }
            Event::Edited(edited) => match edited.id.as_str() {
                "feed" => self.feed_input = edited.value,
                "query" => {
                    self.dictionary.query = edited.value;
                    self.dictionary.again = true;
                    self.dictionary.chosen = None;
                }
                "sheet-query" => {
                    self.coverage.adding = Some(edited.value);
                    self.coverage.found = None;
                }
                "group-name" => self.coverage.renaming = Some(edited.value),
                _ => {}
            },
            _ => {}
        }
    }

    fn command(&mut self, invocation: commands::Invocation) -> Result<(), String> {
        if !invocation.is(&["hfind"]) {
            return Ok(());
        }
        let query = invocation.word("name").unwrap_or_default();
        let query = query.trim();
        if query.chars().count() < names::MIN_QUERY {
            return Err(words(
                "Type a name after the command, in the game's language or translated. For example: /hfind Mythrite Ingot",
                "Напишите название после команды, в оригинале или в переводе. Например: /hfind Mythrite Ingot",
            ));
        }
        if self.names_ready() {
            self.search(query, None, true);
        } else if self.dictionary.failed.is_some() {
            return Err(words(
                "The name dictionary could not be built; the reason is in Harmonia's log.",
                "Словарь названий не удалось собрать; причина в журнале Harmonia.",
            ));
        } else {
            self.dictionary.chat = Some(query.to_owned());
            let _ = chat::Message::new()
                .muted(words(
                    "Preparing the dictionary; the answer follows.",
                    "Словарь готовится; ответ будет следом.",
                ))
                .print();
        }
        Ok(())
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
                let input: FeedsInput =
                    serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                let checked: Vec<Checked> = input
                    .feeds
                    .into_iter()
                    .map(|url| check_feed(url, input.testing))
                    .collect();
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
                serde_json::to_vec(&compile_job(input)?)
            }
            "catalog" => {
                let hash: String = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                let text = catalog::build(&hash)?.to_lines()?;
                storage::write(&catalog::path(&hash), text.as_bytes()).map_err(|e| e.message)?;
                Ok(text.into_bytes())
            }
            "names" => {
                let wanted: names::Build =
                    serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                let (lines, built) = names::build(&wanted)?;
                let _ = jobs::report(0.95, &words("Saving", "Сохранение"));
                let mut text = wanted.header();
                for line in lines {
                    text.push('\n');
                    text.push_str(&line);
                }
                storage::write(names::INDEX, text.as_bytes()).map_err(|e| e.message)?;
                serde_json::to_vec(&built)
            }
            "search" => {
                let input: SearchInput =
                    serde_json::from_slice(&input).map_err(|e| e.to_string())?;
                let bytes = library::read_quietly(names::INDEX, 256 * 1024 * 1024)?;
                let index = String::from_utf8_lossy(&bytes);
                let body = index.split_once('\n').map_or("", |(_, body)| body);
                serde_json::to_vec(&names::search(
                    body,
                    &input.query,
                    input.category,
                    input.limit,
                ))
            }
            _ => return Err(format!("no job {name}")),
        };
        output.map_err(|e| e.to_string())
    }
}

/// Builds the table of a translation with what the player keeps in the
/// game's language, and its fonts.
fn compile_job(input: CompileInput) -> Result<Stamp, String> {
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
    let groups = input.coverage.groups();
    let (table, summary) = compile::compile(&pack, |sheet, layout| {
        coverage::sheet_keep(&groups, sheet, layout, || {
            Ok(angelica_sdk::excel::sheet(sheet)
                .map(|info| info.fields.into_iter().map(|f| (f.name, f.index)).collect())
                .unwrap_or_default())
        })
    })?;
    let _ = jobs::report(0.8, &words("Preparing fonts", "Подготовка шрифтов"));
    let (fonts, font_reports) = prepare_fonts(&pack, &input.hash)?;
    let _ = jobs::report(0.9, &words("Saving", "Сохранение"));
    storage::write(&library::table_path(&input.hash), &table).map_err(|e| e.message)?;
    let stamp = Stamp {
        hash: input.hash,
        coverage: input.coverage.fingerprint(),
        summary,
        game_version: runtime::game_version(),
        fonts,
        font_reports,
    };
    library::save_stamp(&stamp)?;
    Ok(stamp)
}

/// Patched font files, as game path and storage path, and a report per
/// target.
type PreparedFonts = (Vec<(String, String)>, Vec<font_patch::Report>);

/// Patches the game's font files with the pack's glyphs and saves the
/// changed files in storage.
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
fn check_feed(url: String, testing: bool) -> Checked {
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
            release: feed.newest(testing).cloned(),
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
    inspection
        .translation
        .changelog
        .clone_from(&release.changelog);
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
            changelog: None,
        },
    })
}

angelica_sdk::plugin!(Harmonia);
