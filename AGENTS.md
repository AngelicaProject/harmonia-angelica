# Harmonia for Angelica

Harmonia as an Angelica plugin: a Rust crate built as a `wasm32-wasip2` component against `angelica-sdk` from a checkout of the angelica repository beside this one (`../angelica`). Its rules for plugins, the plugin API, and permissions live in that repository's `docs/plugins/` and `docs/formats/plugin-api-v1.md`.

## Layout

- `src/pack.rs` reads and fully verifies packs: digest, signature, every record and string (the same rules as HarmoniaEngine's `Packs/Hpk/`; the format belongs to Aeria).
- `src/compile.rs` turns a pack into an Angelica text table, the rule file of the game map's `sheet_text` event (`lens.decide-file`, under the `text` domain); the host applies its rules to every row the game builds, with the layout, client language, and source guard checks. It leaves out what stays in the game's language, sheet by sheet or column by column (the pack stores each cell's column). Cyrillic letter case is rules at `letter_upper` and `letter_lower`.
- `src/coverage.rs` keeps the same twelve sheet groups and split-sheet column indices as HarmoniaEngine. Groups filter the tree. The saved translation choices apply to sheets, folders, and their name or description columns independently of group membership. Older group choices remain effective until the player overrides them.
- `src/catalog.rs` builds the pack catalogue and previews in a job. `src/coverage_browser.rs` builds windows into the scrolling tree, filters, mark states, and bulk actions outside the frame loop. The original for a sample is available when Angelica names the column.
- `src/names.rs` is the name dictionary (the rules of HarmoniaEngine's `Dictionary/NameIndex.cs` and `NameText.cs`): an index built in a job from the game's sheets, the applied translation, and the languages the player chose, kept in storage, and searched in a job. A category whose sheet the game map gives no `name` column is left out until the map names it.
- `src/library.rs` keeps installed translations and settings in the plugin's storage.
- `src/fonts.rs`, `src/fdt.rs`, `src/atlas.rs`, and `src/font_patch.rs` port HarmoniaEngine's font glyph rules (the same sets, candidate pages, packing, and Shift-JIS codes) onto the game's files read through Angelica. The host serves the patched files and checks them against the game's originals; prepared files are made again when the game version changes.
- `src/feed.rs` reads feeds (the same rules as HarmoniaEngine's `Feeds/FeedDocument.cs`) and checks a downloaded release against its feed entry.
- `src/lib.rs` is the plugin: import from the storage folder's `import/`, feed links the player adds (checked 30 seconds after start and then at the interval the player chose; a release downloads in a job; a feed's first release waits for the player's confirmation of the publisher, and a later release signed by the same key installs by itself, unless the player turned that off, and replaces the earlier one), choosing a translation, preparing its table and font files in a job, applying both at game start with Cyrillic letter case, and `/hfind` in chat. Player-visible text comes in English and Russian by the language of Angelica's interface.
- The window's tabs are `src/tab_translation.rs` (the state in large text, the translations, release notes, and on the first run Project Prima as a suggestion; its key is known, so it installs without the publisher question), `src/tab_dictionary.rs` (names with the game's icons; the chosen name can be copied or opened on the Console Games Wiki by its English name or on Garland Tools by its row), `src/tab_coverage.rs`, and `src/tab_settings.rs` (includes a collapsed diagnostics section for the game version and update checks). A frame of the game gives a plugin little time and fuel: nothing that grows with a pack's size runs in `tick`, `ui`, or `ui_event`.
- `tests/data/` holds the interop packs Aeria's tests produce (`harmonia-interop*.hpk`); HarmoniaEngine's tests read the same files.

The plugin reaches only the hosts its manifest lists (GitHub pages and release files). The table excludes server names and command syntax. Angelica's host uses verified game map entries to keep quest Say phrases and accept original action names in commands. Compatibility profiles, the window language, and the context menu entry of HarmoniaEngine are not part of it.

## Standby

Angelica manages plugin activation for the current game session. Use an Angelica host with [standby support](https://github.com/AngelicaProject/angelica/blob/main/docs/architecture/loading.md#coexistence) when both plugin runtimes are installed.

## Rules

- Like HarmoniaEngine, translations change only at game start.
- Log what the plugin does and why something failed: the player sends the log to the translation's author with **Copy report** in the Angelica menu.
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass before a commit.
