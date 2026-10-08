//! Harmonia in a host with a scripted game: a pack goes from the import
//! folder to translated rows at the next game start.
use angelica_fake_game::{TestHost, build_component_in, row_buffer, row_strings, store_row};
use angelica_text::table::{Column, Variant};
use runtime::{
    excel_text::SheetInfo,
    host::{Action, Event, Status},
    sandbox::{
        Limits,
        bindings::angelica::plugin::ui::{Widget, WidgetRef},
    },
};
use std::{path::Path, sync::OnceLock, time::Duration};

const MANIFEST: &str = include_str!("../angelica.toml");
const INTEROP: &str = "../Harmonia.Tests/TestData/harmonia-interop.hpk";

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component_in(Path::new(env!("CARGO_MANIFEST_DIR")), "harmonia"))
}

fn start(test: &mut TestHost) {
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
    test.host.plugin("harmonia").unwrap().windows()[0]
        .widgets
        .iter()
        .filter_map(|widget| match widget {
            Widget::Text(shown) => Some(shown.text.clone()),
            _ => None,
        })
        .collect()
}

fn has_progress(test: &TestHost) -> bool {
    test.host.plugin("harmonia").unwrap().windows()[0]
        .widgets
        .iter()
        .any(|widget| matches!(widget, Widget::Progress(_)))
}

/// Runs frames until the running job ends.
fn wait(test: &TestHost) {
    test.frames(1);
    for _ in 0..6000 {
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
    assert!(
        texts(&test)
            .iter()
            .any(|t| t.starts_with("Aeria tests — ru translation from en")),
        "{:?}",
        texts(&test)
    );
    click(&test, "install");
    wait(&test);
    assert!(
        texts(&test)
            .iter()
            .any(|t| t == "Installed Interop test pack 2026.10.01.0001."),
        "{:?}",
        texts(&test)
    );
    assert!(!import.join("interop.hpk").exists());

    click(&test, "use-0");
    wait(&test);
    let shown = texts(&test);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("Ready; it applies at the next game start.")),
        "{shown:?}"
    );
    assert!(shown.contains(&"No translation is applied in this session.".to_owned()));

    // The next game start.
    let folder = test.folder;
    let mut test = TestHost::in_folder(folder, Limits::default());
    start(&mut test);
    let shown = texts(&test);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("Interop test pack 2026.10.01.0001 (ru) is applied")),
        "{shown:?}"
    );

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
    test.frames(61);
    assert!(
        texts(&test)
            .iter()
            .any(|t| t.starts_with("Translated so far: 2 rows, 3 cells.")),
        "{:?}",
        texts(&test)
    );
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
    assert!(
        texts(&test)
            .iter()
            .any(|t| t.contains("is applied in this session")),
        "{:?}",
        texts(&test)
    );
}
