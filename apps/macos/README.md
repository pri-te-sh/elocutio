# Elocutio for macOS

The first platform shell: a menu-bar writing companion built on the forked
Harper desktop overlay, rebranded as Elocutio and extended with an OpenRouter
BYOK path.

**Status: M0 partial.** The app builds and runs, the local overlay works in
TextEdit, Notes, and Safari, and an OpenRouter key validates. Chrome and Mail
are not yet supported; system-wide AI rewrite is M1. See
[`docs/M0-VERIFICATION.md`](../../docs/M0-VERIFICATION.md) for the evidence and
known gaps.

## Requirements

- macOS 14+ (tested on macOS 26.6.2 arm64)
- Xcode Command Line Tools
- Rust 1.98.1 (pinned by `rust-toolchain.toml`)
- Node.js 22+, pnpm 11+
- Accessibility permission for Elocutio

Full Xcode is required for Developer ID signing/notarization, not for this
debug build.

## Build and run

From the repository root:

```bash
./scripts/fetch-upstream.sh   # only needed to inspect the full upstream tree
pnpm install
pnpm --filter @elocutio/macos build
cd apps/macos
pnpm exec tauri build --debug --bundles app
open src-tauri/target/debug/bundle/macos/Elocutio.app
```

Development loop:

```bash
cd apps/macos
pnpm tauri dev
```

The debug app is unsigned. On first launch:

1. Open **System Settings → Privacy & Security → Accessibility**.
2. Enable **Elocutio**.
3. Restart the app (or press **Start suggestions** in the setup window).

A rebuild can change the ad-hoc code signature and cause macOS to ask again.
A stable development certificate (`just dev-cert`) is recommended if TCC
resets become annoying.

## Provider key

The setup window accepts an OpenRouter API key. It is stored in the macOS
Keychain at service `app.elocutio.provider.openrouter`, account `api_key`; it
is not written to `config.json` and is never logged. **Test key** calls
`GET /api/v1/key`; **Rewrite with OpenRouter** proves the provider path inside
the app.

## Layout

```text
apps/macos/
├── src/                    # minimal Svelte setup/status UI (M0)
├── public/                 # light/dark lockups for the UI
├── test/overlay.html       # controlled Safari/Chrome AX test target
└── src-tauri/
    ├── src/
    │   ├── main.rs         # Tauri main + highlighter subprocess
    │   ├── openrouter.rs   # Keychain + OpenRouter key/rewrite (M0)
    │   ├── mac_broker/     # Accessibility broker (inherited; audited)
    │   ├── highlighter/    # egui overlay and suggestion cards
    │   ├── tray.rs
    │   ├── windows.rs
    │   └── config/         # JSON config, integrations, dictionary
    ├── icons/              # generated from assets/icons
    ├── tauri.conf.json
    └── Cargo.toml          # standalone workspace; vendored harper-core paths
```

## What M0 changed

- Rebranded product name, bundle ID `app.elocutio.desktop`, icons, tray menu,
  window titles, issue URL, and config directory (`~/Library/Application Support/elocutio`).
- Vendored the minimal Harper crate subset under `vendor/harper/` and added
  `elocutio-llm` + `elocutio-platform` dependencies.
- Replaced the inherited Svelte settings UI with a small Svelte 5 setup/status
  shell. The full settings UI is M1 work.
- Added Keychain-backed OpenRouter key validation, an in-app rewrite, and a
  system-wide **Rewrite Selection** tray action using pasteboard + ⌘V with
  read-back verification and clipboard restore.
- Added macOS notifications for rewrite success/failure.
- Added the floating Harper-style suggestion widget and the LLM-cache bridge into the overlay card.
- Added the structured analysis pipeline: paragraph chunking, prompt/schema,
  JSON validation, exact/normalized/fuzzy span resolution, dedupe, and rejected-edit reporting.
- Added provider/model persistence and a local metadata-only usage/cost log.
- Added a real onboarding checklist (Accessibility, provider key, shortcut)
  that persists `onboarding_completed` so setup no longer reopens every launch.
- Added the global **⌥⌘E** shortcut, transient/concealed pasteboard writes, and
  clipboard text restore.
- Marshalled Accessibility and pasteboard work to the macOS main thread and
  whitelisted overlay apps for M0 stability.
- Removed the inherited updater for M0; it returns in M5 with our own minisign
  keypair.
- Added AX tree-walk bounds and activation scoping so large Mail/Chrome trees
  cannot freeze the highlighter.
- Added a controlled overlay test page at `test/overlay.html`.

## Known gaps

- **Chrome:** the AX activation reaches text-range bounds but no overlay is
  drawn; test with Grammarly's extension disabled and treat the MV3 extension
  (M3) as the real browser path.
- **Mail:** exposes a supported text element but no usable text-range bounds.
  Keep it out of the supported matrix until the fallback lands.
- **Shortcut configuration:** ⌥⌘E is fixed; make it configurable in settings.
- **Clipboard fidelity:** only text is restored after the paste; non-text
  pasteboard items are replaced. Use a safe item reconstruction path in M1.
- **Settings depth:** onboarding, model persistence, and usage are in place;
  per-app enablement, privacy controls, budget warnings, and voice settings
  remain M1/M2.
- **Usage scope:** the log is local and metadata-only; the Usage card sums all
  entries, not a calendar month.
- **LLM overlay:** validated LLM suggestions merge into the focused field’s
  inherited card via the cache bridge; the floating widget opens that card.
  Ranking local vs. LLM suggestions and an in-widget Analyze action remain.
- **Unsigned debug build:** no notarization, updater, or release packaging yet.

## Tests and checks

```bash
# from repository root
just check
just macos-check

# or directly
cd apps/macos
pnpm check
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
```

The controlled overlay target:

```bash
python3 -m http.server 8765 --bind 127.0.0.1 --directory apps/macos/test
open -a Safari "http://127.0.0.1:8765/overlay.html"
open -a "Google Chrome" "http://127.0.0.1:8765/overlay.html"
```

## Upstream

The vendored subset is pinned in `upstream.lock`. It is intended to be read-only;
carry Elocutio changes in `apps/macos` or a documented patch, not by silently
editing vendored code. See [`vendor/harper/README.md`](../../vendor/harper/README.md)
and [`UPSTREAM.md`](../../UPSTREAM.md).
