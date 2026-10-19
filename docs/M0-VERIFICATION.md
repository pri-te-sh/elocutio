# M0 verification — fork, rebrand, spike

**Date:** 2026-10-19
**Machine:** macOS 26.6.2 (25G83), arm64
**Build:** debug `.app` (unsigned), `apps/macos/src-tauri/target/debug/bundle/macos/Elocutio.app`
**Upstream:** Harper v2.13.0, commit `f4ebc61d77df1fc00b6aa8bc604e91af3534dad1`
**Result:** **Partial.** The rebranded app builds and runs, the local overlay and click-to-replace work in native AppKit apps, OpenRouter key validation works, and the system-wide **Rewrite Selection** write ladder (pasteboard + ⌘V, read-back verification) works in TextEdit, Safari, and Chrome. Chrome and Mail do not yet produce an overlay, and there is no global shortcut yet.

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
- [x] A clean `git archive HEAD` export passes `pnpm install --frozen-lockfile`, `svelte-check`, and `cargo check` against the committed vendored tree.

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
- [x] **System-wide AI rewrite** replaces a selection through the pasteboard + ⌘V write ladder with read-back verification.
- [x] The prior clipboard text is restored after the paste.
- [x] A macOS notification reports success or failure.
- [ ] A global keyboard shortcut is not wired yet; the action is in the tray menu.

The click path uses the AX fast path (`element.set_value`) for local lints. The explicit rewrite uses the pasteboard + ⌘V ladder, which is the required path for Chromium/Electron.

## Write-ladder matrix

Method: select text, choose **Elocutio → Rewrite Selection** from the menu-bar menu, and check that the selected text is replaced.

| App | Result | Notes |
| --- | --- | --- |
| **TextEdit** | **Pass** | Selection replaced; notification shown. |
| **Safari** | **Pass** | Local test page textarea replaced. |
| **Chrome** | **Pass** | Local test page textarea replaced even though the overlay does not render in Chrome. |
| **Mail** | Not tested | Overlay is already unsupported; paste replacement may work, but do not claim it until tested. |

The write ladder is independent of the overlay: it reads `AXSelectedText`, writes the replacement to the pasteboard, synthesizes ⌘V, waits 180 ms, verifies the replacement in `AXValue`, and restores the prior clipboard text. Selection length is capped at 12,000 characters. Secure input is refused via `IsSecureEventInputEnabled()` and `AXSubrole == AXSecureTextField`.

## OpenRouter

- [x] Key is stored in the macOS Keychain under service `app.elocutio.provider.openrouter`, account `api_key`.
- [x] Key is **not** present in `config.json` or logs.
- [x] `test_openrouter_key()` calls `GET https://openrouter.ai/api/v1/key` and the user confirmed it was accepted.
- [x] In-app `rewrite_text()` returned text for the sample sentence (user confirmed).
- [x] System-wide selection rewrite through the write ladder works in TextEdit, Safari, and Chrome (user confirmed).
- [ ] The default model `anthropic/claude-haiku-5.5` was not separately verified; the rewrite succeeded with the user's chosen model.

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
3. **Global shortcut:** add a configurable shortcut for Rewrite Selection; the tray action is the current entry point.
4. **Clipboard fidelity:** the current implementation restores text only. Preserve non-text pasteboard items or use a transient pasteboard type for the rewrite.
5. **Undo/focus regression tests:** add a manual script and, if possible, an automated AX harness.
6. **Signed build:** M0 uses an unsigned debug `.app`. Full Xcode and Developer ID signing are required for TCC-stable distribution.
7. **Updater:** removed for M0. Re-add in M5 with a real minisign keypair, never Harper's.
8. **Onboarding:** the developer shell opens setup on every launch until onboarding is completed; wire that in M1.

## Notes on M0 completion

The M0 acceptance gate has two parts still open: the five-app overlay matrix (3/5) and the inherited security audit. The write-ladder and key-validation gates now pass. Keep M0 as **in progress** in the tracker until Chrome/Mail and the audit are resolved.

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

# Write ladder: select text in TextEdit/Safari/Chrome, then
# Elocutio menu bar → Rewrite Selection
```

Screen recording permission was declined during testing, so there are no screenshots. All matrix results above are user-reported interactive checks plus highlighter logs and process samples.
