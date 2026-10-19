# M0 verification — fork, rebrand, spike

**Date:** 2026-10-19
**Machine:** macOS 26.6.2 (25G83), arm64
**Build:** debug `.app` (unsigned), `apps/macos/src-tauri/target/debug/bundle/macos/Elocutio.app`
**Upstream:** Harper v2.13.0, commit `f4ebc61d77df1fc00b6aa8bc604e91af3534dad1`
**Result:** **Partial.** The rebranded app builds and runs, the local overlay and click-to-replace work in native AppKit apps, and OpenRouter key validation plus an in-app rewrite work. Chrome and Mail do not yet produce an overlay; system-wide AI rewrite is not implemented.

## Build and launch

```bash
./scripts/fetch-upstream.sh
pnpm install
pnpm --filter @elocutio/macos build
cd apps/macos
pnpm exec tauri build --debug --bundles app
open src-tauri/target/debug/bundle/macos/Elocutio.app
```

Verified:

- [x] `cargo check --manifest-path apps/macos/src-tauri/Cargo.toml` passes.
- [x] `cargo test --manifest-path apps/macos/src-tauri/Cargo.toml` passes (75 tests).
- [x] `pnpm --filter @elocutio/macos check` passes.
- [x] `pnpm --filter @elocutio/macos build` passes.
- [x] `pnpm exec tauri build --debug --bundles app` produces `Elocutio.app`.
- [x] App launches as a menu-bar agent; first-run setup window opens.
- [x] Accessibility permission is granted; highlighter subprocess starts.
- [x] Config is written to `~/Library/Application Support/elocutio/`, not the Harper path.

## Overlay matrix

Test method: type a deliberate typo (`tehre`) into a focused text field and look for the Elocutio underline and suggestion card. Safari and Chrome use a local page with a textarea and a contenteditable.

| App | Result | Notes |
| --- | --- | --- |
| **TextEdit** | **Pass** | Underline and card appear; clicking the spelling suggestion replaces `tehre` → `there`; click is stable after the AX tree-walk bounds fix. |
| **Notes** | **Pass** | Underline and card appear; suggestions apply. |
| **Safari** | **Pass** | Local test page textarea/contenteditable show the overlay. The browser extension remains the planned robust path. |
| **Chrome** | **Fail / partial** | Accessibility activation reaches `FoundTextRangeBounds`, but the overlay does not appear on the test page. A Grammarly extension is installed and may alter the AX/DOM tree; also needs a browser-extension path (M3). |
| **Mail** | **Fail** | Compose body exposes a supported text element but never produces usable text-range bounds, so no overlay. Bounded activation and tree walks prevent it from freezing the highlighter. |

**Acceptance status:** 3 of 5 apps pass. M0's original five-app gate is **not met**.

## Click, focus, and injection

- [x] Clicking a spelling suggestion in TextEdit replaces the intended span.
- [x] The replacement no longer freezes the app after the AX tree-walk bounds fix.
- [x] User confirmed focus stays in TextEdit and the replacement is correct.
- [ ] Undo was not explicitly verified in this pass.
- [ ] System-wide **AI rewrite** replacing a selection is **not implemented**. The current write path applies local Harper lints through `AXUIElementSetAttributeValue`; the pasteboard + ⌘V ladder is still M1 work.

The click path currently uses the AX fast path (`element.set_value`). The pasteboard + ⌘V fallback is required for Chromium/Electron and remains to be built.

## OpenRouter

- [x] Key is stored in the macOS Keychain under service `app.elocutio.provider.openrouter`, account `api_key`.
- [x] Key is **not** present in `config.json` or logs.
- [x] `test_openrouter_key()` calls `GET https://openrouter.ai/api/v1/key` and the user confirmed it was accepted.
- [x] In-app `rewrite_text()` returned text for the sample sentence (user confirmed).
- [ ] System-wide selection rewrite through the write ladder is not implemented.
- [ ] The default model `anthropic/claude-haiku-5.5` was not separately verified; the in-app rewrite succeeded with the user's chosen model.

## Brand and dark mode

- [x] Product name: **Elocutio**; bundle ID: `app.elocutio.desktop`.
- [x] App icon, `.icns`, `.ico`, menu-bar template, and social preview generated from the new mark.
- [x] Tray/window titles, issue URL, config path, and highlighter labels rebranded.
- [x] Brand guide v1.1 applied: **Soft Indigo `#B8BEFF`** on dark surfaces; **never Ink Indigo on dark**.
- [x] Settings UI uses a white lockup in dark mode and Soft Indigo accents.
- [x] User confirmed dark mode “looks right.”
- [x] No user-facing Harper branding remains; internal crate names (`harper-core`, `harper-core` dependencies) remain for attribution and are documented in `NOTICE`/`UPSTREAM.md`.

## Known issues and next actions

1. **Mail:** expose usable text-range bounds (or fall back to the static-text collector) before claiming support. Until then, keep Mail out of the supported matrix.
2. **Chrome:** test with Grammarly disabled; build the MV3 extension + local bridge (M3) as the reliable browser path. Do not claim Chrome support from the AX path.
3. **System-wide AI rewrite:** implement the pasteboard + ⌘V write ladder with read-back verification; the current app can only rewrite inside its own test card.
4. **Undo/focus regression tests:** add a manual script and, if possible, an automated AX harness.
5. **Signed build:** M0 uses an unsigned debug `.app`. Full Xcode and Developer ID signing are required for TCC-stable distribution.
6. **Updater:** removed for M0. Re-add in M5 with a real minisign keypair, never Harper's.
7. **Onboarding:** the developer shell opens setup on every launch until onboarding is completed; wire that in M1.

## Commands used

```bash
# root checks
just check

# macOS app
cd apps/macos
pnpm check
pnpm build
pnpm exec tauri build --debug --bundles app

# controlled Safari/Chrome target
python3 -m http.server 8765 --bind 127.0.0.1 --directory apps/macos/test
open -a Safari "http://127.0.0.1:8765/overlay.html"
open -a "Google Chrome" "http://127.0.0.1:8765/overlay.html"

# Mail compose target
osascript -e 'tell application "Mail" to make new outgoing message with properties {visible:true, content:"This is tehre test."}'
```

Screen recording permission was declined during testing, so there are no screenshots. All matrix results above are user-reported interactive checks plus highlighter logs and process samples.
