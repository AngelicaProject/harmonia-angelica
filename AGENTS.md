# Harmonia for Angelica

Harmonia as an Angelica plugin: a Rust crate built as a `wasm32-wasip2` component against `angelica-sdk` from a checkout of the angelica repository beside this one (`../angelica`). Its rules for plugins, the plugin API, and permissions live in that repository's `docs/plugins/` and `docs/formats/plugin-api-v1.md`.

## Layout

- `src/pack.rs` reads and fully verifies packs: digest, signature, every record and string (the same rules as HarmoniaEngine's `Packs/Hpk/`; the format belongs to Aeria).
- `src/compile.rs` turns a pack into an Angelica text table; the host applies it to every row the game builds, with the layout, client language, and source guard checks.
- `src/library.rs` keeps installed translations, settings, and the session count in the plugin's storage.
- `src/fonts.rs`, `src/fdt.rs`, `src/atlas.rs`, and `src/font_patch.rs` port HarmoniaEngine's font glyph rules (the same sets, candidate pages, packing, and Shift-JIS codes) onto the game's files read through Angelica. The host serves the patched files and checks them against the game's originals; prepared files are made again when the game version changes.
- `src/feed.rs` reads feeds (the same rules as HarmoniaEngine's `Feeds/FeedDocument.cs`) and checks a downloaded release against its feed entry.
- `src/lib.rs` is the plugin: import from the storage folder's `import/`, feed links the player adds (checked 30 seconds after start and every hour; a release downloads in a job; a feed's first release waits for the player's confirmation of the publisher, and a later release signed by the same key installs by itself and replaces the earlier one), choosing a translation, preparing its table and font files in a job, and applying both at game start with Cyrillic letter case. Player-visible text comes in English and Russian by the language of Angelica's interface.
- `tests/data/` holds the interop packs Aeria's tests produce (`harmonia-interop*.hpk`); HarmoniaEngine's tests read the same files.

The plugin reaches only the hosts its manifest lists (GitHub pages and release files). The testing channel, the name dictionary, and compatibility profiles of HarmoniaEngine are not part of it.

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
