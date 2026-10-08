//! Harmonia for Angelica: applies Harmonia translation packs (`.hpk`) to the
//! game's text.
//!
//! The player puts a pack into the plugin's `import` folder; a job verifies
//! it completely (digest, signature, every record and string) and, after
//! the player confirms the publisher, installs it. Choosing a translation
//! builds its text table in a job. At the next game start Harmonia
//! activates the table, so the host translates every row the game builds,
//! and adds Cyrillic letters to the game's letter case when the
//! translation is written in Cyrillic.
//!
//! Like the Dalamud build, Harmonia changes translations only at game
//! start: the game copies text into its own caches, so a change in the
//! middle of a session would show a mix.
pub mod compile;
pub mod library;
pub mod pack;

use angelica_sdk::{
    Plugin,
    api::{excel_text, jobs, runtime, text_case},
    ui::{Event, Tone, Window},
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
}

#[derive(Serialize, Deserialize)]
struct InstallInput {
    path: String,
    hash: String,
}

#[derive(Serialize, Deserialize)]
struct CompileInput {
    hash: String,
    kept_sheets: Vec<String>,
}

enum Job {
    Inspect,
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

    /// Whether the table of `hash` was built with the current settings.
    fn table_is_current(&self, hash: &str) -> bool {
        matches!(library::load_stamp(hash), Ok(Some(stamp)) if stamp.kept_sheets == self.settings.kept_sheets)
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
                self.library.push(translation);
                self.imports = library::imports();
                self.save();
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

    fn click(&mut self, id: &str) {
        if let Some(index) = id
            .strip_prefix("check-")
            .and_then(|i| i.parse::<usize>().ok())
        {
            if let Some((path, _)) = self.imports.get(index).cloned() {
                self.inspection = None;
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
                    if let Some(inspection) = &self.inspection {
                        let input = InstallInput {
                            path: inspection.path.clone(),
                            hash: inspection.translation.hash.clone(),
                        };
                        self.inspection = None;
                        self.start_job("install", &input, Job::Install);
                    }
                }
                "cancel" => self.inspection = None,
                _ => {}
            }
        }
    }

    fn status_lines(&self, window: Window) -> Window {
        let applied = self
            .applied
            .as_deref()
            .and_then(|hash| self.translation(hash));
        let chosen = self
            .settings
            .active
            .as_deref()
            .and_then(|hash| self.translation(hash));
        let mut window = match applied {
            Some(translation) => window.toned(
                format!(
                    "{} {} ({}) is applied in this session.",
                    translation.title, translation.version, translation.language
                ),
                Tone::Good,
            ),
            None => window.text("No translation is applied in this session."),
        };
        if let Some(status) = &self.status {
            window = window.muted(format!(
                "Translated so far: {} rows, {} cells. Left in the original because the game's text changed: {}.",
                status.rows_changed, status.cells_replaced, status.guard_mismatches
            ));
            if status.rows_refused > 0 {
                window = window.toned(
                    format!(
                        "Rows with an unexpected layout, left untouched: {}.",
                        status.rows_refused
                    ),
                    Tone::Warning,
                );
            }
            if !status.sheets_mismatched.is_empty() {
                let names: Vec<_> = status
                    .sheets_mismatched
                    .iter()
                    .take(8)
                    .map(|m| m.sheet.as_str())
                    .collect();
                window = window.toned(
                    format!(
                        "Sheets that differ in this game build, left untouched: {} ({}).",
                        status.sheets_mismatched.len(),
                        names.join(", ")
                    ),
                    Tone::Warning,
                );
            }
        }
        if self.applied != self.settings.active {
            window = window.toned(
                match chosen {
                    Some(translation) => {
                        format!("{} applies at the next game start.", translation.title)
                    }
                    None => "The game stays untranslated from the next start.".into(),
                },
                Tone::Warning,
            );
        }
        if !self.started_with_game {
            window = window.toned(
                "Harmonia was enabled after the game started; translations apply when the game starts.",
                Tone::Warning,
            );
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
        };
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
        let mut window = Window::new(WINDOW, "Harmonia").heading("This session");
        window = self
            .status_lines(window)
            .separator()
            .heading("Translations");
        if self.library.is_empty() {
            window = window.muted("No translation is installed.");
        }
        for (index, translation) in self.library.iter().enumerate() {
            let chosen = self.settings.active.as_deref() == Some(translation.hash.as_str());
            let key = translation.key.as_deref().map_or_else(
                || "unsigned".to_owned(),
                |key| format!("key {}", short(key)),
            );
            window = window
                .text(format!(
                    "{} {} — {}",
                    translation.title, translation.version, translation.team
                ))
                .muted(format!(
                    "{} from {}, {} cells; game {}; {key}",
                    translation.language,
                    translation.game_language,
                    translation.cells,
                    translation.game_version
                ))
                .row(|row| {
                    row.button_enabled(
                        format!("use-{index}"),
                        if chosen { "Chosen" } else { "Use" },
                        !chosen && self.job.is_none(),
                    )
                    .button_enabled(
                        format!("remove-{index}"),
                        "Remove",
                        self.job.is_none(),
                    )
                });
        }
        if self.settings.active.is_some() {
            window = window.button_enabled("none", "Use no translation", self.job.is_none());
        }
        window = window.separator().heading("Add a translation").muted(
            "Put a .hpk file into the import folder of Harmonia's plugin folder (Angelica menu, Open plugin folder), then check it here.",
        );
        for (index, (path, size)) in self.imports.iter().enumerate() {
            window = window.row(|row| {
                row.text(format!("{path} ({})", megabytes(*size)))
                    .button_enabled(format!("check-{index}"), "Check", self.job.is_none())
            });
        }
        window = window.button("refresh", "Look again");
        if let Some(inspection) = &self.inspection {
            let translation = &inspection.translation;
            window = window
                .separator()
                .heading(format!("{} {}", translation.title, translation.version))
                .text(format!(
                    "{} — {} translation from {}, {} cells in {} sheets, made for game {}.",
                    translation.team,
                    translation.language,
                    translation.game_language,
                    translation.cells,
                    translation.sheets,
                    translation.game_version
                ));
            window = match &translation.key {
                Some(key) => window.text(format!(
                    "Signed by key {}. Install only if you trust this publisher.",
                    short(key)
                )),
                None => window.toned(
                    "This pack is not signed: anyone could have made or changed it.",
                    Tone::Warning,
                ),
            };
            window = window.row(|row| row.button("install", "Install").button("cancel", "Cancel"));
        }
        if self.job.is_some() {
            window = window.progress(self.progress.0, &self.progress.1);
        }
        if !self.message.0.is_empty() {
            window = window.toned(&self.message.0, self.message.1);
        }
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
                angelica_sdk::api::storage::rename(
                    &input.path,
                    &inspection.translation.pack_path(),
                )
                .map_err(|e| e.message)?;
                serde_json::to_vec(&inspection.translation)
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
                let _ = jobs::report(0.9, "Saving");
                angelica_sdk::api::storage::write(&library::table_path(&input.hash), &table)
                    .map_err(|e| e.message)?;
                let stamp = Stamp {
                    hash: input.hash,
                    kept_sheets: input.kept_sheets,
                    summary,
                };
                library::save_stamp(&stamp)?;
                serde_json::to_vec(&stamp)
            }
            _ => return Err(format!("no job {name}")),
        };
        output.map_err(|e| e.to_string())
    }
}

/// Verifies a pack completely and describes it.
fn inspect(path: &str) -> Result<Inspection, String> {
    let bytes = library::read_file(path, pack::MAX_PACK)?;
    let _ = jobs::report(0.4, "Checking the pack");
    let pack = pack::Pack::parse(&bytes).map_err(|e| format!("{path}: {}", e.0))?;
    if compile::game_language(&pack.manifest.game.language).is_none() {
        return Err(format!(
            "{path}: the game has no language {}",
            pack.manifest.game.language
        ));
    }
    let manifest = &pack.manifest;
    Ok(Inspection {
        path: path.to_owned(),
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
        },
    })
}

angelica_sdk::plugin!(Harmonia);
