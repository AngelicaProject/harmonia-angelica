//! Harmonia in a host with a scripted game: a pack goes from the import
//! folder to translated rows at the next game start.
use angelica_fake_game::ui::{Edited, Widget, WidgetRef};
use angelica_fake_game::{TestHost, build_component_in, row_buffer, row_strings, store_row};
use angelica_text::table::{Column, Variant};
use runtime::{
    excel_text::SheetInfo,
    host::{Action, Event, Status},
    sandbox::Limits,
};
use std::{path::Path, sync::OnceLock, time::Duration};

const MANIFEST: &str = include_str!("../angelica.toml");
const INTEROP: &str = "tests/data/harmonia-interop.hpk";

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component_in(Path::new(env!("CARGO_MANIFEST_DIR")), "harmonia"))
}

/// The game map's events Harmonia decides at: the game's sheet text and
/// its letter case conversions.
const TEXT_MAP: [(&str, &str); 4] = [
    (
        "entries/excel.toml",
        r#"
[function."excel.hash_table_page_row_resolver.store_row"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr", "ptr"], ret = "u8" }
thread = "any"
"#,
    ),
    (
        "entries/text.toml",
        r#"
[function."text.utf8_string.to_upper"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u8", "u8", "u8", "ptr"], ret = "ptr" }
thread = "any"

[function."text.utf8_string.to_lower"]
locator = { signature = "C3" }
abi = { args = ["ptr"], ret = "ptr" }
thread = "any"
"#,
    ),
    (
        "domains.toml",
        r#"
[domains.text]
doc = "The game's text."
class = "display"
read = { en = "Sees the game's text", ru = "Видит тексты игры" }
change = { en = "Changes the game's text", ru = "Меняет тексты игры" }
"#,
    ),
    (
        "events.toml",
        r#"
[events.sheet_text]
doc = "A text cell of a sheet row the game builds."
at = "excel.hash_table_page_row_resolver.store_row"
derived = "sheet_cell"
domain = "text"
rules = "text-table-v1"

[events.sheet_text.args.sheet]
doc = "The sheet."
from = "sheet"
text = 256

[events.sheet_text.outcomes.replace]
doc = "The cell shows the rule's text."
text = 65535

[events.letter_upper]
doc = "A letter the conversion to capitals meets."
at = "text.utf8_string.to_upper"
derived = "case_letter"
domain = "text"

[events.letter_upper.args.letter]
doc = "The letter."
from = "letter"
text = 4

[events.letter_upper.outcomes.becomes]
doc = "The rule's letter."
text = 4
same_length = "letter"

[events.letter_lower]
doc = "A letter the conversion to small letters meets."
at = "text.utf8_string.to_lower"
derived = "case_letter"
domain = "text"

[events.letter_lower.args.letter]
doc = "The letter."
from = "letter"
text = 4

[events.letter_lower.outcomes.becomes]
doc = "The rule's letter."
text = 4
same_length = "letter"
"#,
    ),
];

fn start(test: &mut TestHost) {
    test.use_map(&TEXT_MAP, &[]);
    test.install(MANIFEST, component()).unwrap();
    test.host.start_all();
    if test.host.plugin("harmonia").unwrap().status() == Status::AwaitingApproval {
        test.host.queue(Action::Approve("harmonia".into()));
        test.frames(1);
    }
    assert_eq!(
        test.host.plugin("harmonia").unwrap().status(),
        Status::Running
    );
    test.event("harmonia", Event::OpenRequested);
}

fn click(test: &TestHost, id: &str) {
    test.event(
        "harmonia",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: id.into(),
        }),
    );
}

fn texts(test: &TestHost) -> Vec<String> {
    let mut shown = Vec::new();
    for widget in &test.host.plugin("harmonia").unwrap().windows()[0].widgets {
        match widget {
            Widget::Text(line) => shown.push(line.text.clone()),
            Widget::Entry(entry) => {
                shown.extend([entry.title.clone(), entry.note.clone()]);
                shown.push(format!("{:?}", entry.mark));
                shown.extend(
                    entry
                        .facts
                        .iter()
                        .map(|fact| format!("{}: {}", fact.name, fact.value)),
                );
            }
            _ => {}
        }
    }
    shown
}

/// The question the window asks: the dialog's id, then its title, texts,
/// and facts as `name: value`.
fn question(test: &TestHost) -> Vec<String> {
    let windows = test.host.plugin("harmonia").unwrap().windows();
    let Some(dialog) = windows.first().and_then(|window| window.dialog.as_ref()) else {
        return Vec::new();
    };
    let mut shown = vec![dialog.id.clone(), dialog.title.clone()];
    for widget in &dialog.widgets {
        match widget {
            Widget::Text(line) => shown.push(line.text.clone()),
            Widget::Fact(fact) => shown.push(format!("{}: {}", fact.name, fact.value)),
            _ => {}
        }
    }
    shown
}

fn has_progress(test: &TestHost) -> bool {
    test.host.plugin("harmonia").unwrap().windows()[0]
        .widgets
        .iter()
        .any(|widget| match widget {
            Widget::Meter(_) => true,
            // A row shows the work on its own translation or feed.
            Widget::Entry(entry) => entry.progress.is_some(),
            _ => false,
        })
}

/// Runs frames until the running job ends.
fn wait(test: &TestHost) {
    wait_up_to(test, Duration::from_mins(1));
}

/// Waits for the plugin's work to end, at most `limit`.
fn wait_up_to(test: &TestHost, limit: Duration) {
    test.frames(1);
    let started = std::time::Instant::now();
    while started.elapsed() < limit {
        if !has_progress(test) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        test.frames(1);
    }
    panic!("the job did not finish: {:?}", texts(test));
}

fn addon() -> SheetInfo {
    SheetInfo {
        name: "Addon".into(),
        variant: Variant::Rows,
        language: 2,
        columns: vec![
            Column {
                index: 0,
                offset: 4,
            },
            Column {
                index: 2,
                offset: 8,
            },
        ],
    }
}

fn quest() -> SheetInfo {
    SheetInfo {
        name: "quest/000/Test".into(),
        variant: Variant::Subrows,
        language: 2,
        columns: vec![Column {
            index: 1,
            offset: 0,
        }],
    }
}

#[test]
fn a_pack_is_checked_installed_chosen_and_applied_at_the_next_start() {
    let mut test = TestHost::new(Limits::default());
    let import = test.folder.path().join("storage/harmonia/import");
    std::fs::create_dir_all(&import).unwrap();
    std::fs::copy(INTEROP, import.join("interop.hpk")).unwrap();
    start(&mut test);

    click(&test, "check-0");
    wait(&test);
    let asked = question(&test);
    assert_eq!(
        asked.first().map(String::as_str),
        Some("install"),
        "{asked:?}"
    );
    assert!(asked.iter().any(|t| t == "Team: Aeria tests"), "{asked:?}");
    assert!(asked.iter().any(|t| t == "Language: ru ← en"), "{asked:?}");
    // Installing is choosing: the pack becomes the translation in use and is
    // prepared, with no separate Use.
    click(&test, "install");
    wait(&test);
    assert!(!import.join("interop.hpk").exists());
    let shown = texts(&test);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("Ready; it applies at the next game start.")),
        "{shown:?}"
    );
    assert!(shown.contains(&"Restart the game to turn on Interop test pack.".to_owned()));

    // The next game start.
    let folder = test.folder;
    let mut test = TestHost::in_folder(folder, Limits::default());
    start(&mut test);
    let shown = texts(&test);
    assert!(shown.iter().any(|t| t == "On"), "{shown:?}");

    let excel = &test.services().excel;
    let columns = addon().columns;
    let row = store_row(
        excel,
        &addon(),
        1,
        0,
        row_buffer(12, &columns, &[b"Hello", b"World"]),
        12,
    );
    assert_eq!(row_strings(&row, 12)[1], "Мир".as_bytes());
    let row = store_row(
        excel,
        &addon(),
        7,
        0,
        row_buffer(12, &columns, &[b"x", b"Hello"]),
        12,
    );
    assert_eq!(
        row_strings(&row, 12)[1],
        b"Hello",
        "the game's text changed"
    );
    let quest_columns = quest().columns;
    let row = store_row(
        excel,
        &quest(),
        3,
        1,
        row_buffer(4, &quest_columns, &[b"Second"]),
        4,
    );
    assert_eq!(row_strings(&row, 4)[0], "Вторая".as_bytes());

    let mut name = "паладин".as_bytes().to_vec();
    test.services()
        .case
        .letters()
        .to_upper(&mut name, true, false, |_| false);
    assert_eq!(name, "Паладин".as_bytes(), "ru is written in Cyrillic");

    // Harmonia refreshes the counters once a second of frames.
    click(&test, "row-t0");
    test.frames(61);
    assert!(
        texts(&test)
            .iter()
            .any(|t| t.starts_with("Translated so far: 3 cells.")),
        "{:?}",
        texts(&test)
    );

    // Removing asks first; Cancel keeps the translation.
    click(&test, "remove-0");
    test.frames(1);
    assert_eq!(question(&test).first().map(String::as_str), Some("remove"));
    click(&test, "remove-no");
    test.frames(1);
    assert!(question(&test).is_empty());
    assert!(texts(&test).iter().any(|t| t == "Interop test pack"));
    click(&test, "remove-0");
    click(&test, "remove-yes");
    test.frames(1);
    assert!(question(&test).is_empty());
    assert!(!texts(&test).iter().any(|t| t == "Interop test pack"));
}

/// Whether the Addon row 1 comes out translated.
fn translates(test: &TestHost) -> bool {
    let columns = addon().columns;
    let row = store_row(
        &test.services().excel,
        &addon(),
        1,
        0,
        row_buffer(12, &columns, &[b"Hello", b"World"]),
        12,
    );
    row_strings(&row, 12)[1] == "Мир".as_bytes()
}

#[test]
fn harmonia_in_dalamud_that_translated_the_previous_session_keeps_this_copy_off() {
    let mut test = TestHost::new(Limits::default());
    let storage = test.folder.path().join("storage/harmonia");
    std::fs::create_dir_all(storage.join("import")).unwrap();
    std::fs::copy(INTEROP, storage.join("import/interop.hpk")).unwrap();
    start(&mut test);
    click(&test, "check-0");
    wait(&test);
    click(&test, "install");
    wait(&test);
    click(&test, "use-0");
    wait(&test);
    let version = test.services().game.version();

    // The plugin was approved in a running game, so session 1 is the next
    // game start. It translates; Harmonia in Dalamud then answers it.
    let mut test = TestHost::in_folder(test.folder, Limits::default());
    start(&mut test);
    assert!(translates(&test));
    let answer = |session: u64, game_version: &str| {
        std::fs::write(
            storage.join("dalamud.json"),
            format!(
                r#"{{"format": 1, "translating": true, "game_version": "{game_version}", "angelica_session": {session}}}"#
            ),
        )
        .unwrap();
    };
    answer(1, &version);

    let mut test = TestHost::in_folder(test.folder, Limits::default());
    start(&mut test);
    assert!(
        !translates(&test),
        "Harmonia in Dalamud translated session 1"
    );
    assert!(
        texts(&test)
            .iter()
            .any(|t| t
                .starts_with("Harmonia in Dalamud translated the game in the previous session")),
        "{:?}",
        texts(&test)
    );

    // No answer in session 2: Dalamud did not load.
    let mut test = TestHost::in_folder(test.folder, Limits::default());
    start(&mut test);
    assert!(translates(&test), "nothing answered session 2");

    // An answer from another game build does not count.
    answer(3, "2000.01.01.0000.0000");
    let mut test = TestHost::in_folder(test.folder, Limits::default());
    start(&mut test);
    assert!(translates(&test), "the answer is for another game build");
}

#[test]
fn a_damaged_pack_is_refused() {
    let mut test = TestHost::new(Limits::default());
    let import = test.folder.path().join("storage/harmonia/import");
    std::fs::create_dir_all(&import).unwrap();
    let mut bytes = std::fs::read(INTEROP).unwrap();
    bytes[200] ^= 1;
    std::fs::write(import.join("damaged.hpk"), bytes).unwrap();
    start(&mut test);
    click(&test, "check-0");
    wait(&test);
    assert!(
        texts(&test)
            .iter()
            .any(|t| t.contains("import/damaged.hpk:")),
        "{:?}",
        texts(&test)
    );
}

/// A real translation, as players use it: `HARMONIA_PACK` names the file.
#[test]
#[ignore = "needs a pack named by HARMONIA_PACK"]
fn a_real_pack_compiles_within_the_job_limits() {
    let pack = std::env::var("HARMONIA_PACK").expect("HARMONIA_PACK");
    let mut test = TestHost::new(Limits::default());
    let import = test.folder.path().join("storage/harmonia/import");
    std::fs::create_dir_all(&import).unwrap();
    std::fs::copy(&pack, import.join("real.hpk")).unwrap();
    start(&mut test);
    let started = std::time::Instant::now();
    click(&test, "check-0");
    wait(&test);
    eprintln!("checked in {:?}: {:?}", started.elapsed(), texts(&test));
    click(&test, "install");
    wait(&test);
    let started = std::time::Instant::now();
    click(&test, "use-0");
    wait(&test);
    eprintln!("prepared in {:?}: {:?}", started.elapsed(), texts(&test));
    assert!(
        texts(&test).iter().any(|t| t.starts_with("Ready")),
        "{:?}",
        texts(&test)
    );

    let folder = test.folder;
    let mut test = TestHost::in_folder(folder, Limits::default());
    let started = std::time::Instant::now();
    start(&mut test);
    eprintln!("applied in {:?}: {:?}", started.elapsed(), texts(&test));
    assert!(texts(&test).iter().any(|t| t == "On"), "{:?}", texts(&test));
}

/// The game's font files for the targets of the Aeria fonts pack: a table
/// with matching metrics and seven empty textures per set.
fn font_game(pack: &[u8]) -> angelica_fake_game::game_files::FakeGame {
    let pack = harmonia::pack::Pack::parse(pack).unwrap();
    let mut game = angelica_fake_game::game_files::FakeGame::new("2026.09.15.0000.0000");
    let sections = [pack.fonts.as_ref(), pack.font_replacements.as_ref()];
    for suffix in ["", "_lobby"] {
        for target in sections.iter().flatten().flat_map(|fonts| &fonts.targets) {
            let mut table = b"fcsv0100".to_vec();
            table.extend_from_slice(&0x20u32.to_le_bytes());
            table.extend_from_slice(&0x40u32.to_le_bytes());
            table.resize(0x20, 0);
            table.extend_from_slice(b"fthd");
            table.extend_from_slice(&[0; 12]);
            table.extend_from_slice(&256u16.to_le_bytes());
            table.extend_from_slice(&256u16.to_le_bytes());
            table.extend_from_slice(&12f32.to_le_bytes());
            table.extend_from_slice(&i32::from(target.line_height).to_le_bytes());
            table.extend_from_slice(&i32::from(target.ascent).to_le_bytes());
            table.extend_from_slice(b"knhd");
            table.extend_from_slice(&[0; 12]);
            game = game.with_file(format!("common/font/{}{suffix}.fdt", target.name()), table);
        }
        let prefix = if suffix.is_empty() {
            "font"
        } else {
            "font_lobby"
        };
        for number in 1..=7 {
            let mut texture = vec![0u8; 80 + 256 * 256 * 2];
            texture[4..8].copy_from_slice(&0x1440u32.to_le_bytes());
            texture[8..10].copy_from_slice(&256u16.to_le_bytes());
            texture[10..12].copy_from_slice(&256u16.to_le_bytes());
            texture[28..32].copy_from_slice(&80u32.to_le_bytes());
            game = game.with_file(format!("common/font/{prefix}{number}.tex"), texture);
        }
    }
    game
}

#[test]
fn a_pack_with_fonts_replaces_the_games_font_files_at_the_next_start() {
    const FONTS: &str = "tests/data/harmonia-interop-fonts.hpk";
    let pack = std::fs::read(FONTS).unwrap();
    let mut test = TestHost::new(Limits::default());
    font_game(&pack).write(&test.game_folder()).unwrap();
    let import = test.folder.path().join("storage/harmonia/import");
    std::fs::create_dir_all(&import).unwrap();
    std::fs::write(import.join("fonts.hpk"), &pack).unwrap();
    start(&mut test);
    click(&test, "check-0");
    wait(&test);
    click(&test, "install");
    wait(&test);
    assert!(
        texts(&test).iter().any(|t| t.starts_with("Ready")),
        "{:?}",
        texts(&test)
    );

    let folder = test.folder;
    let mut test = TestHost::in_folder(folder, Limits::default());
    start(&mut test);
    click(&test, "row-t0");
    test.frames(1);
    let shown = texts(&test);
    assert!(shown.iter().any(|t| t.starts_with("Fonts: ")), "{shown:?}");
    let target = harmonia::pack::Pack::parse(&pack)
        .unwrap()
        .fonts
        .unwrap()
        .targets[0]
        .name();
    assert!(
        test.services()
            .files
            .lookup(
                format!("common/font/{target}.fdt")
                    .to_lowercase()
                    .as_bytes()
            )
            .is_some()
    );
}

/// The whole path from a feed link to an installed translation, against a
/// feed published on the network.
#[test]
fn a_pack_a_previous_game_downloaded_is_picked_up_at_the_next_start() {
    let mut test = TestHost::new(Limits::default());
    let downloads = test.folder.path().join("storage/harmonia/downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::copy(INTEROP, downloads.join("pack.hpk")).unwrap();
    std::fs::write(
        downloads.join("pack.json"),
        r#"{"feed": "https://example.org/feed-v1.json", "key": null}"#,
    )
    .unwrap();
    start(&mut test);
    wait(&test);
    // Its feed declared no key, so the player decides; the pack is not lost.
    let asked = question(&test);
    assert_eq!(
        asked.first().map(String::as_str),
        Some("install"),
        "{asked:?}"
    );
    assert!(asked.iter().any(|t| t == "Team: Aeria tests"), "{asked:?}");
    click(&test, "install");
    wait(&test);
    let shown = texts(&test);
    assert!(
        shown
            .iter()
            .any(|t| t == "Restart the game to turn on Interop test pack."),
        "{shown:?}"
    );
    assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 0);
}

#[test]
#[ignore = "needs the network and a feed link in HARMONIA_FEED"]
fn a_feed_release_is_downloaded_checked_and_installed() {
    let feed = std::env::var("HARMONIA_FEED").unwrap();
    let mut test = TestHost::new(Limits {
        memory_bytes: 1 << 30,
        ..Limits::default()
    });
    start(&mut test);
    test.event(
        "harmonia",
        Event::Edited(Edited {
            window: "main".into(),
            id: "feed".into(),
            value: feed,
        }),
    );
    click(&test, "feed-add");
    wait(&test);
    // The feed's row: its newest version, and its size to download.
    let shown = texts(&test);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("20") && t.matches('.').count() == 3),
        "{shown:?}"
    );

    // One Download: the pack signed with the key the feed declares installs,
    // becomes the chosen translation, and is prepared, with no second
    // decision; one restart remains.
    click(&test, "feed-get-0");
    // As long as the host lets a download take.
    wait_up_to(&test, Duration::from_mins(15));
    let shown = texts(&test);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("Restart the game to turn on ")),
        "{shown:?}"
    );
    assert!(
        !shown.iter().any(|t| t.starts_with("Signed by key ")),
        "{shown:?}"
    );
    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(test.folder.path().join("storage/harmonia/settings.json")).unwrap(),
    )
    .unwrap();
    assert!(settings["active"].is_string(), "{settings}");
    click(&test, "row-t0");
    test.frames(1);
    let shown = texts(&test);
    assert!(
        shown.contains(&"Updates: Up to date.".to_owned()),
        "{shown:?}"
    );
    let storage = test.folder.path().join("storage/harmonia");
    assert_eq!(std::fs::read_dir(storage.join("packs")).unwrap().count(), 1);
    assert_eq!(
        std::fs::read_dir(storage.join("downloads"))
            .unwrap()
            .count(),
        0
    );
}
