# Harmonia for Angelica

Harmonia as an Angelica plugin: a Rust crate built as a `wasm32-wasip2` component against `angelica-sdk` from a checkout of the angelica repository beside this one (`../angelica`). Its rules for plugins, the plugin API, and permissions live in that repository's `docs/plugins/` and `docs/formats/plugin-api-v1.md`.

## Layout

- `src/pack.rs` reads and fully verifies packs: digest, signature, every record and string (the same rules as HarmoniaEngine's `Packs/Hpk/`; the format belongs to Aeria).
- `src/compile.rs` turns a pack into an Angelica text table, the rule file of the game map's `sheet_text` event (`lens.decide-file`, under the `text` domain); the host applies its rules to every row the game builds, with the layout, client language, and source guard checks. It leaves out what stays in the game's language, sheet by sheet or column by column (the pack stores each cell's column). Cyrillic letter case is rules at `letter_upper` and `letter_lower`.
- `src/coverage.rs` is what stays in the game's language: the ten groups of HarmoniaEngine's `Packs/SheetGroups.cs`, which the player renames, changes, removes, and restores, and the player's own groups. A group has one choice for its names and one for its descriptions; an entry is a sheet (`Item`), a column (`Item:description`), or a folder (`quest/*`), and a column can be set apart from its group. Names and descriptions are told apart by the column names the game map gives (`excel.sheet`), with a list of sheets of names for the columns the map does not name; any other column is a description. A column without a name in the map cannot be set on its own.
- `src/catalog.rs` lists what a pack holds, sheet by sheet, with samples and quest names, for the Coverage tab; it is built in a job, kept as one line per sheet, and read a few hundred lines a frame, since a large pack has thousands of sheets.
- `src/names.rs` is the name dictionary (the rules of HarmoniaEngine's `Dictionary/NameIndex.cs` and `NameText.cs`): an index built in a job from the game's sheets, the applied translation, and the languages the player chose, kept in storage, and searched in a job. A category whose sheet the game map gives no `name` column is left out until the map names it.
- `src/library.rs` keeps installed translations, settings, and the session count in the plugin's storage.
- `src/fonts.rs`, `src/fdt.rs`, `src/atlas.rs`, and `src/font_patch.rs` port HarmoniaEngine's font glyph rules (the same sets, candidate pages, packing, and Shift-JIS codes) onto the game's files read through Angelica. The host serves the patched files and checks them against the game's originals; prepared files are made again when the game version changes.
- `src/feed.rs` reads feeds (the same rules as HarmoniaEngine's `Feeds/FeedDocument.cs`) and checks a downloaded release against its feed entry.
- `src/lib.rs` is the plugin: import from the storage folder's `import/`, feed links the player adds (checked 30 seconds after start and then at the interval the player chose; a release downloads in a job; a feed's first release waits for the player's confirmation of the publisher, and a later release signed by the same key installs by itself, unless the player turned that off, and replaces the earlier one), choosing a translation, preparing its table and font files in a job, applying both at game start with Cyrillic letter case, and `/hfind` in chat. Player-visible text comes in English and Russian by the language of Angelica's interface.
- The window's tabs are `src/tab_translation.rs` (the state in large text, the translations, release notes, and on the first run Project Prima as a suggestion; its key is known, so it installs without the publisher question), `src/tab_dictionary.rs` (names with the game's icons; the chosen one copies, tries on what can be worn, and opens its page on the Console Games Wiki by its English name or on Garland Tools by its row), `src/tab_coverage.rs`, and `src/tab_settings.rs`. A frame of the game gives a plugin little time and fuel: nothing that grows with a pack's size runs in `tick`, `ui`, or `ui_event`.
- `tests/data/` holds the interop packs Aeria's tests produce (`harmonia-interop*.hpk`); HarmoniaEngine's tests read the same files.

The plugin reaches only the hosts its manifest lists (GitHub pages and release files). Compatibility profiles, the window language, quest Say phrases, and the context menu entry of HarmoniaEngine are not part of it.

## Stepping aside for HarmoniaEngine

The game is translated once, and HarmoniaEngine comes first, because it also keeps what other Dalamud plugins read. The contract between the two builds:

- At every game start (`runtime::started_with_game`), this plugin increments `session` in `session.json` of its storage folder (`storage/harmonia/` in the Angelica installation).
- HarmoniaEngine, once its row hooks are installed, finds the installation through `angelica.json` in the game folder and writes `dalamud.json` beside it: `{"format": 1, "translating": true, "game_version": "<ffxivgame.ver>", "angelica_session": <session it read>}` (`Game/AngelicaHandoff.cs` there).
- This plugin stays off for a session when `dalamud.json` answered the previous session on the same game build (`library::begin_session`). Without an answer, as when Dalamud does not load after a game update, it translates from the next start. A damaged answer counts as none.

Change both repositories together when the contract changes.

## Rules

- Like HarmoniaEngine, translations change only at game start.
- Log what the plugin does and why something failed: the player sends the log to the translation's author with **Copy report** in the Angelica menu.
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass before a commit.
