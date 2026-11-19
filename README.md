# Harmonia for Angelica

Harmonia shows FINAL FANTASY XIV in another language using community translations. This repository is Harmonia as an [Angelica](https://github.com/AngelicaProject/angelica) plugin: it runs in Angelica's sandbox inside the game and keeps working after a game update as soon as the game map is updated.

Harmonia is also a Dalamud plugin, [HarmoniaEngine](https://github.com/AngelicaProject/Harmonia). The two are developed side by side. Angelica hosts with standby support block their plugins when Dalamud starts with the game. When XIVLauncher skips Dalamud, Angelica starts its enabled plugins as usual.

## Using it

1. Install Harmonia from the Angelica menu.
2. Open Harmonia and press **Install** on the suggested translation, or paste the link the translation's author gave you and press **Add**, or put the translation file (`.hpk`) into Harmonia's `import` folder (the window's menu, **Open folder**).
3. Restart the game.

Type `/harmonia` in chat to open or close Harmonia.

Translations added from a link update themselves, unless you turn that off in **Settings**. **Coverage** shows the pack's sheet tree in one scrollable list. Search by sheet, folder, or quest, then check what stays in the game's language. Click a group name to open it. Its checkbox keeps the entire group in the game’s language. Open **Filters** to narrow it to names, descriptions, or sheets you left in the game's language. The **...** menu on a sheet opens its examples and group settings. **Dictionary**, and `/hfind` in chat, find a name in the game's language and in the translation; a click on one shows it in the other languages, with buttons to copy it or open its page on a game wiki. Changes take effect at the next game start. If something goes wrong, use **Copy report for the author** on Harmonia in the Angelica menu and send it to the translation's author.

**Translation** shows whether the pack targets the running game version. Open a pack’s card for its target version, contents, publisher key, and replacement details. **Settings → Diagnostics** shows the client version and the last update check.

The default names the game sends to the server when buying an apartment or private chamber or registering a company airship or submersible stay in the client's language. After updating Harmonia, let the active translation finish preparing and restart the game to apply the rebuilt table.

With a supporting Angelica runtime and game map, the host also restores original item, key item, and quest labels in outgoing chat links. Linked effects are sent by ID so recipients see the game's own effect text in their language. The message you type stays as written.

Game command names, aliases, and parameter tokens stay in the client's language. Command descriptions can still be translated.

On builds supported by the Angelica runtime and game map, commands such as `/micon` also accept original action names. Recognized quests that require a phrase in Say keep that phrase in the client's language. If you type its translation in Say at the required quest step with a suitable target loaded, the original phrase is sent.

## For developers

Requirements: the Rust toolchain in `rust-toolchain.toml` and a checkout of the angelica repository next to this one (`../angelica`), which provides the SDK until it is published.

```text
cargo test
cargo build --release --target wasm32-wasip2
```

`tests/host.rs` runs the plugin in the Angelica host against a scripted game. `HARMONIA_PACK=<pack> cargo test --release --test host -- --ignored` also runs it with a real pack, and `HARMONIA_FEED=<feed link> cargo test --release --test host a_feed_release -- --ignored` with a published feed over the network.

CI checks every push and pull request (`.github/workflows/ci.yml`). A release is a tag `v<version>` on `main` matching `plugin.version` in `angelica.toml`: `.github/workflows/release.yml` builds the component, signs it with the author key (the repository secret `ANGELICA_AUTHOR_KEY`), publishes it in the release of the tag, and behind the link that stays the same across versions, `https://github.com/AngelicaProject/harmonia-angelica/releases/download/harmonia/harmonia.angelica`. Installed copies update from that link.

Translation packs are made with [Aeria](https://github.com/AngelicaProject/Aeria), which specifies the pack and feed formats. How the plugin works and the rules for changing it are in [AGENTS.md](AGENTS.md).

## License

Copyright (C) 2026 pokeda. Licensed under [AGPL-3.0-or-later](LICENSE.md), with the author attribution term in [NOTICE](NOTICE).
