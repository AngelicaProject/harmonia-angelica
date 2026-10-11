# Harmonia for Angelica

Harmonia shows FINAL FANTASY XIV in another language using community translations. This repository is Harmonia as an [Angelica](https://github.com/AngelicaProject/angelica) plugin: it runs in Angelica's sandbox inside the game and keeps working after a game update as soon as the game map is updated.

Harmonia is also a Dalamud plugin, [HarmoniaEngine](https://github.com/AngelicaProject/Harmonia). The two are developed side by side. When both are installed, the Dalamud build translates and this one stays off; this one translates on the days the game runs without Dalamud, such as after a game update.

## Using it

1. Install Harmonia from the Angelica menu.
2. Open Harmonia, paste the link the translation's author gave you, and press **Add**, or put the translation file (`.hpk`) into Harmonia's `import` folder (the window's menu, **Open folder**).
3. Restart the game.

Translations added from a link update themselves. Changes take effect at the next game start. If something goes wrong, use **Copy report** on Harmonia in the Angelica menu and send it to the translation's author.

## For developers

Requirements: the Rust toolchain in `rust-toolchain.toml` and a checkout of the angelica repository next to this one (`../angelica`), which provides the SDK until it is published.

```text
cargo test
cargo build --release --target wasm32-wasip2
```

`tests/host.rs` runs the plugin in the Angelica host against a scripted game. `HARMONIA_PACK=<pack> cargo test --release --test host -- --ignored` also runs it with a real pack, and `HARMONIA_FEED=<feed link> cargo test --release --test host a_feed_release -- --ignored` with a published feed over the network.

CI checks every push and pull request (`.github/workflows/ci.yml`). A release is a tag `v<version>` on `main` matching `plugin.version` in `angelica.toml`: `.github/workflows/release.yml` builds the component, signs it with the author key (the repository secret `ANGELICA_AUTHOR_KEY`), and publishes it behind the link that stays the same across versions, `https://github.com/AngelicaProject/harmonia-angelica/releases/download/harmonia/harmonia.angelica`. Installed copies update from it.

Translation packs are made with [Aeria](https://github.com/AngelicaProject/Aeria), which specifies the pack and feed formats. How the plugin works and the rules for changing it are in [AGENTS.md](AGENTS.md).

## License

[AGPL-3.0-or-later](LICENSE.md).
