//! Harmonia in a host with a scripted game: a pack goes from the import
//! folder to translated rows at the next game start.
use angelica_fake_game::ui::{Edited, Tone, Widget, WidgetRef};
use angelica_fake_game::{
    TestHost, build_component_in,
    game_files::{FakeGame, TextSheet},
    row_buffer, row_strings, store_row,
};
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
                shown.extend([entry.title.clone(), entry.note.clone(), entry.value.clone()]);
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
    assert!(shown.iter().any(|t| t == "Translation ready."), "{shown:?}");
    assert!(shown.contains(&"Restart the game to turn on Interop test pack.".to_owned()));

    // The next game start.
    let folder = test.folder;
    let mut test = TestHost::in_folder(folder, Limits::default());
    start(&mut test);
    let shown = texts(&test);
    assert!(shown.iter().any(|t| t == "On"), "{shown:?}");
    assert!(
        shown.iter().any(|t| t == "For a different game version"),
        "{shown:?}"
    );
    click(&test, "row-t0");
    assert!(
        texts(&test)
            .iter()
            .any(|text| text.starts_with("Pack contents: "))
    );
    choose_tab(&test, 2);
    let pending = || {
        test.host.plugin("harmonia").unwrap().windows()[0]
            .status
            .as_ref()
            .is_some_and(|status| status.tone == Tone::Warning)
    };
    assert!(!pending());
    test.toggle("harmonia", "main", "groups/1", true);
    assert!(pending());
    test.toggle("harmonia", "main", "groups/1", false);
    assert!(!pending());
    choose_tab(&test, 0);

    check_applied_pack_diagnostics(&test);

    // Removing asks first; Cancel keeps the translation.
    click(&test, "remove-0");
    test.frames(1);
    assert_eq!(question(&test).first().map(String::as_str), Some("remove"));
    click(&test, "remove-no");
    test.frames(1);
    assert!(question(&test).is_empty());
    assert!(
        texts(&test)
            .iter()
            .any(|t| t.starts_with("Interop test pack · "))
    );
    click(&test, "remove-0");
    click(&test, "remove-yes");
    test.frames(1);
    assert!(question(&test).is_empty());
    assert!(
        !texts(&test)
            .iter()
            .any(|t| t.starts_with("Interop test pack"))
    );
}

fn check_applied_pack_diagnostics(test: &TestHost) {
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

    // Actual replacements appear in the pack card without changing the version status.
    test.frames(61);
    let shown = texts(test);
    assert!(
        shown.iter().any(|t| t == "For a different game version"),
        "{shown:?}"
    );
    let plugin = test.host.plugin("harmonia").unwrap();
    assert!(
        plugin.windows()[0]
            .widgets
            .iter()
            .all(|widget| !matches!(widget, Widget::Readout(_) | Widget::Meter(_)))
    );
    assert!(shown.iter().any(|text| text == "Applied replacements: 3"));
    choose_tab(test, 3);
    let settings = plugin.windows();
    let mut diagnostics = settings[0]
        .widgets
        .iter()
        .skip_while(|widget| {
            !matches!(
                widget,
                Widget::BeginSection(section)
                    if section.id == "diagnostics" && !section.open_by_default
            )
        })
        .take_while(|widget| !matches!(widget, Widget::EndSection));
    assert!(diagnostics.any(|widget| matches!(
        widget,
        Widget::Fact(fact) if fact.name == "Game version"
    )));
    assert!(!settings[0].widgets.iter().any(|widget| matches!(
        widget,
        Widget::Fact(fact) if fact.name == "Applied replacements"
    )));
    choose_tab(test, 0);
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
fn harmonia_translates_on_the_first_normal_launch_with_an_old_dalamud_marker() {
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

    std::fs::write(storage.join("session.json"), r#"{"session":1}"#).unwrap();
    std::fs::write(
        storage.join("dalamud.json"),
        format!(
            r#"{{"format":1,"translating":true,"game_version":"{version}","angelica_session":1}}"#
        ),
    )
    .unwrap();

    let mut test = TestHost::in_folder(test.folder, Limits::default());
    test.use_map(&TEXT_MAP, &[]);
    test.install(MANIFEST, component()).unwrap();
    test.host.watch_standby(|| true);
    test.host.start_all();
    assert!(!translates(&test));

    let mut test = TestHost::in_folder(test.folder, Limits::default());
    start(&mut test);
    assert!(translates(&test));
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
        texts(&test).iter().any(|t| t == "Translation ready."),
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
        texts(&test).iter().any(|t| t == "Translation ready."),
        "{:?}",
        texts(&test)
    );

    let folder = test.folder;
    let mut test = TestHost::in_folder(folder, Limits::default());
    start(&mut test);
    click(&test, "row-t0");
    let plugin = test.host.plugin("harmonia").unwrap();
    assert!(plugin.windows()[0].widgets.iter().any(|widget| matches!(
        widget,
        Widget::Entry(entry) if entry.id == "row-t0" && entry.facts.iter().any(|fact|
            fact.name == "Replaced font files" && fact.value.parse::<u64>().is_ok_and(|n| n > 0))
    )));
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
    assert!(shown.iter().any(|t| t.ends_with(" MB")), "{shown:?}");

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
        shown
            .iter()
            .any(|t| t.starts_with("Updates: ") && t.contains("Up to date.")),
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

/// The game files of a test: the command list `/hfind` needs, and sheets.
fn game_with(sheets: &[(&str, &TextSheet)]) -> FakeGame {
    let mut game = FakeGame::new("2026.09.15.0000.0000").with_text(
        "TextCommand",
        &TextSheet::new(2, &[0, 1]).row(1, &[(0, "/tell"), (1, "/t")]),
    );
    for (name, sheet) in sheets {
        game = game.with_text(*name, sheet);
    }
    game
}

fn choose_tab(test: &TestHost, tab: u32) {
    test.choose("harmonia", "main", "tab", tab);
}

/// Runs frames until `done`, at most a minute.
fn until(test: &TestHost, done: impl Fn(&TestHost) -> bool) {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_mins(1) {
        test.frames(1);
        if done(test) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("not done: {:?}", texts(test));
}

/// Installs the interop pack from the import folder and chooses it.
fn install_interop(test: &mut TestHost) {
    let import = test.folder.path().join("storage/harmonia/import");
    std::fs::create_dir_all(&import).unwrap();
    std::fs::copy(INTEROP, import.join("interop.hpk")).unwrap();
    test.event("harmonia", Event::OpenRequested);
    click(test, "check-0");
    wait(test);
    click(test, "install");
    wait(test);
}

fn check_coverage_membership(test: &TestHost) {
    click(test, "sheet-details-0");
    assert_eq!(
        question(test).first().map(String::as_str),
        Some("coverage-details")
    );
    click(test, "details-groups");
    assert_eq!(
        question(test).first().map(String::as_str),
        Some("coverage-groups")
    );
    let belongs_to_items = || {
        let plugin = test.host.plugin("harmonia").unwrap();
        let windows = plugin.windows();
        let dialog = windows[0].dialog.as_ref().unwrap();
        dialog.widgets.iter().find_map(|widget| match widget {
            Widget::Checkbox(checkbox) if checkbox.id == "member-items" => Some(checkbox.checked),
            _ => None,
        })
    };
    assert_eq!(belongs_to_items(), Some(false));
    test.toggle("harmonia", "coverage-groups", "member-items", true);
    assert_eq!(belongs_to_items(), Some(true));
    test.toggle("harmonia", "coverage-groups", "member-items", false);
    assert_eq!(belongs_to_items(), Some(false));
    click(test, "membership-done");
}

#[test]
fn a_coverage_group_filters_the_tree_without_owning_its_sheet_marks() {
    // Addon's column 0 is a name and column 2 a description, as the game
    // map names them.
    let addon = TextSheet::new(3, &[0, 2]).row(1, &[(0, "Hello"), (2, "World")]);
    let game = game_with(&[("Addon", &addon)]);
    let mut test = TestHost::new(Limits::default());
    game.write(&test.game_folder()).unwrap();
    test.name_columns("Addon", &[("name", 0), ("description", 2)]);
    start(&mut test);
    install_interop(&mut test);

    choose_tab(&test, 2);
    test.choose("harmonia", "main", "groups", 7);
    let plugin = test.host.plugin("harmonia").unwrap();
    let windows = plugin.windows();
    let sidebar_title = windows[0]
        .widgets
        .iter()
        .find_map(|widget| match widget {
            Widget::BeginSplit(split) => split.labels.get(7),
            _ => None,
        })
        .unwrap();
    assert!(texts(&test).contains(sidebar_title));

    // A group of the player's own with Addon.
    wait(&test);
    click(&test, "new-group");
    test.edit("harmonia", "main", "group-name", "Mine");
    click(&test, "rename-save");
    click(&test, "add-sheets");
    test.edit("harmonia", "main", "sheet-query", "Addon");
    until(&test, |test| texts(test).iter().any(|t| t == "Addon"));
    click(&test, "add-0");
    click(&test, "adding-done");
    until(&test, |test| texts(test).iter().any(|t| t == "Addon"));
    check_coverage_membership(&test);
    click(&test, "coverage-filters");
    assert!(
        test.host.plugin("harmonia").unwrap().windows()[0]
            .widgets
            .iter()
            .any(|widget| matches!(widget, Widget::Choice(choice) if choice.id == "coverage-show"))
    );
    let row_is_checked = |test: &TestHost, expected| {
        test.host.plugin("harmonia").unwrap().windows()[0].widgets.iter().any(|widget| {
            matches!(widget, Widget::Checkbox(row) if row.id == "sheet-mark-0" && row.checked == expected)
        })
    };
    test.toggle("harmonia", "main", "groups/13", true);
    assert!(row_is_checked(&test, true));
    test.toggle("harmonia", "main", "groups/13", false);
    assert!(row_is_checked(&test, false));
    test.toggle("harmonia", "main", "sheet-mark-0", true);
    assert!(row_is_checked(&test, true));
    // The table is prepared again once the player stops changing it.
    test.advance(2.0);
    wait(&test);
    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(test.folder.path().join("storage/harmonia/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["coverage"]["groups"][0]["entries"][0], "Addon");
    assert_eq!(settings["coverage"]["rules"]["Addon"], "original");
    click(&test, "group-actions");
    click(&test, "delete");
    click(&test, "delete-yes");
    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(test.folder.path().join("storage/harmonia/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["coverage"]["groups"], serde_json::json!([]));
    assert_eq!(settings["coverage"]["rules"]["Addon"], "original");

    let folder = test.folder;
    let mut test = TestHost::in_folder(folder, Limits::default());
    test.name_columns("Addon", &[("name", 0), ("description", 2)]);
    start(&mut test);
    assert!(
        !translates(&test),
        "the sheet stays in English after its group is removed"
    );
    let quest_columns = quest().columns;
    let row = store_row(
        &test.services().excel,
        &quest(),
        3,
        1,
        row_buffer(4, &quest_columns, &[b"Second"]),
        4,
    );
    assert_eq!(
        row_strings(&row, 4)[0],
        "Вторая".as_bytes(),
        "other sheets are translated"
    );
}

#[test]
fn harmonia_command_toggles_its_window() {
    let mut test = TestHost::new(Limits::default());
    game_with(&[]).write(&test.game_folder()).unwrap();
    start(&mut test);
    let plugin = test.host.plugin("harmonia").unwrap();
    assert_eq!(plugin.windows().len(), 1);

    assert!(test.command("/harmonia"));
    assert!(plugin.windows().is_empty());
    assert!(test.command("/HARMONIA extra text"));
    assert_eq!(plugin.windows().len(), 1);
    assert!(test.game.printed_text().is_empty());
}

#[test]
fn the_dictionary_finds_names_copies_the_chosen_one_and_answers_hfind() {
    let item = TextSheet::new(2, &[0, 1])
        .row(1, &[(0, "Mythrite Ingot")])
        .row(2, &[(0, "Mythrite Ore")])
        .row(3, &[(0, "Iron Ore")]);
    let game = game_with(&[("Item", &item)]);
    let mut test = TestHost::new(Limits::default());
    game.write(&test.game_folder()).unwrap();
    test.name_columns("Item", &[("name", 0)]);
    start(&mut test);

    choose_tab(&test, 3);
    test.toggle("harmonia", "main", "lang-de", true);
    choose_tab(&test, 1);
    wait(&test);
    test.edit("harmonia", "main", "query", "mythrte");
    until(&test, |test| {
        texts(test).iter().any(|t| t == "Mythrite Ingot")
    });
    let shown = texts(&test);
    assert!(shown.iter().any(|t| t == "Mythrite Ore"), "{shown:?}");
    assert!(!shown.iter().any(|t| t == "Iron Ore"), "{shown:?}");

    // A click chooses the name; copying is a button under it.
    click(&test, "name-0-1");
    assert_eq!(test.services().clipboard.take(), None);
    let shown = texts(&test);
    assert!(
        shown.iter().any(|t| t == "Deutsch: Mythrite Ingot [de]"),
        "{shown:?}"
    );
    click(&test, "copy-o-0-1");
    assert_eq!(
        test.services().clipboard.take().as_deref(),
        Some("Mythrite Ingot")
    );
    assert!(texts(&test).iter().any(|t| t == "copied"));

    test.game.clear_printed();
    assert!(test.command("/hfind iron"));
    until(&test, |test| !test.game.printed_text().is_empty());
    let printed = test.game.printed_text().join("\n");
    assert!(printed.contains("“iron”: 1 found"), "{printed}");
    assert!(printed.contains("Iron Ore"), "{printed}");
}

#[test]
fn the_first_run_suggests_a_translation_and_takes_a_link() {
    let mut test = TestHost::new(Limits::default());
    start(&mut test);
    let shown = texts(&test);
    assert!(shown.iter().any(|t| t == "No translation yet"), "{shown:?}");
    assert!(shown.iter().any(|t| t == "Project Prima"), "{shown:?}");
    let windows = test.host.plugin("harmonia").unwrap().windows();
    assert!(
        windows[0]
            .widgets
            .iter()
            .any(|w| matches!(w, Widget::Button(b) if b.id == "suggest-install"))
    );
    assert!(
        windows[0]
            .widgets
            .iter()
            .any(|w| matches!(w, Widget::TextInput(input) if input.id == "feed"))
    );

    choose_tab(&test, 3);
    let plugin = test.host.plugin("harmonia").unwrap();
    assert!(plugin.windows()[0].widgets.iter().any(|widget| matches!(
        widget,
        Widget::Checkbox(checkbox) if checkbox.id == "auto-install" && checkbox.checked
    )));

    // An explicit choice to turn automatic installation off is saved.
    test.toggle("harmonia", "main", "auto-install", false);
    test.choose("harmonia", "main", "interval", 5);
    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(test.folder.path().join("storage/harmonia/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["auto_install"], false);
    assert_eq!(settings["check_minutes"], 1440);
}

#[test]
#[ignore = "needs a pack named by HARMONIA_PACK"]
fn a_real_packs_sheet_list_loads_within_a_frame() {
    let pack = std::env::var("HARMONIA_PACK").expect("HARMONIA_PACK");
    let mut test = TestHost::new(Limits::default());
    let bytes = std::fs::read(&pack).unwrap();
    let pack = harmonia::pack::Pack::parse(&bytes).unwrap();
    let manifest = &pack.manifest;
    let translation = harmonia::library::Translation {
        hash: pack.hash_hex(),
        title: manifest.title.clone(),
        team: manifest.team.name.clone(),
        version: manifest.version.clone(),
        channel: manifest.channel.clone(),
        language: manifest.language.clone(),
        game_language: manifest.game.language.clone(),
        game_version: manifest.game.version.clone(),
        key: None,
        sheets: pack.sheet_count(),
        cells: pack.cell_count(),
        feed: None,
        changelog: None,
    };
    let storage = test.folder.path().join("storage/harmonia");
    std::fs::create_dir_all(storage.join("packs")).unwrap();
    std::fs::write(storage.join(translation.pack_path()), bytes).unwrap();
    std::fs::write(
        storage.join("translations.json"),
        serde_json::to_vec(&[translation]).unwrap(),
    )
    .unwrap();
    start(&mut test);

    // The sheet list of a real pack loads within a frame's fuel.
    let started = std::time::Instant::now();
    choose_tab(&test, 2);
    until(&test, |test| {
        assert_eq!(
            test.host.plugin("harmonia").unwrap().status(),
            Status::Running
        );
        test.host.plugin("harmonia").unwrap().windows()[0]
            .widgets
            .iter()
            .any(|w| matches!(w, Widget::Checkbox(checkbox) if checkbox.id == "sheet-mark-0"))
    });

    test.choose("harmonia", "main", "coverage-list", 96);
    until(&test, |test| {
        test.host.plugin("harmonia").unwrap().windows()[0].widgets.iter().any(|widget| {
            matches!(widget, Widget::BeginVirtualList(list) if list.first == 96 && list.total > 96)
        })
    });
    let storage = test.folder.path().join("storage/harmonia/tables");
    let size: u64 = std::fs::read_dir(&storage)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".sheets.jsonl"))
        .map(|e| e.metadata().unwrap().len())
        .sum();
    eprintln!("sheet list of {size} bytes in {:?}", started.elapsed());
    assert_eq!(
        test.host.plugin("harmonia").unwrap().status(),
        Status::Running
    );
    let started = std::time::Instant::now();
    test.choose("harmonia", "main", "groups", 10);
    until(&test, |test| {
        assert_eq!(
            test.host.plugin("harmonia").unwrap().status(),
            Status::Running
        );
        let shown = texts(test);
        shown.iter().any(|text| text == "Quest text")
            && test.host.plugin("harmonia").unwrap().windows()[0]
                .widgets
                .iter()
                .any(|w| matches!(w, Widget::Button(button) if button.label.ends_with(" quest")))
    });
    eprintln!(
        "a group shown in {:?}: {:?}",
        started.elapsed(),
        texts(&test)
    );
    assert_eq!(
        test.host.plugin("harmonia").unwrap().status(),
        Status::Running
    );
}
