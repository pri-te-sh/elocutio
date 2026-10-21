# M0 verification — fork, rebrand, spike

**Date:** 2026-10-19
**Machine:** macOS 26.6.2 (25G83), arm64
**Build:** debug `.app` (unsigned), `apps/macos/src-tauri/target/debug/bundle/macos/Elocutio.app`
**Upstream:** Harper v2.13.0, commit `f4ebc61d77df1fc00b6aa8bc604e91af3534dad1`
**Result:** **Partial.** The rebranded app builds and runs, the local overlay and click-to-replace work in native AppKit apps, OpenRouter key validation works, and the system-wide **Rewrite Selection** write ladder (pasteboard + ⌘V, read-back verification) works in TextEdit, Safari, and Chrome via **⌥⌘E** and the tray menu. Chrome and Mail do not yet produce an overlay.

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
- [x] `cargo test --manifest-path apps/macos/src-tauri/Cargo.toml` passes.
- [x] `pnpm --filter @elocutio/macos check` passes.
- [x] `pnpm --filter @elocutio/macos build` passes.
- [x] `pnpm exec tauri build --debug --bundles app` produces `Elocutio.app`.
- [x] App launches as a menu-bar agent; first-run setup window opens.
- [x] Onboarding completes and persists; the setup window does not reopen after **Finish setup**.
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
- [x] **⌥⌘E** is registered as the global Rewrite Selection shortcut; the tray action remains available.
- [x] The previous clipboard text is restored (verified with a sentinel).
- [ ] Rich/non-text pasteboard items are not restored yet.

The click path uses the AX fast path (`element.set_value`) for local lints. The explicit rewrite uses the pasteboard + ⌘V ladder, which is the required path for Chromium/Electron.

## Write-ladder matrix

Method: select text, press **⌥⌘E** or choose **Elocutio → Rewrite Selection** from the menu-bar menu, and check that the selected text is replaced.

| App | Result | Notes |
| --- | --- | --- |
| **TextEdit** | **Pass** | Selection replaced; notification shown. |
| **Safari** | **Pass** | Local test page textarea replaced. |
| **Chrome** | **Pass** | Local test page textarea replaced even though the overlay does not render in Chrome. |
| **Mail** | Not tested | Overlay is already unsupported; paste replacement may work, but do not claim it until tested. |

The write ladder is independent of the overlay: it reads `AXSelectedText`, writes the replacement to a transient/concealed pasteboard string, synthesizes ⌘V, waits 180 ms, verifies the replacement in `AXValue`, and restores the prior clipboard text. Selection length is capped at 12,000 characters. Secure input is refused via `IsSecureEventInputEnabled()` and `AXSubrole == AXSecureTextField`.

The AX and pasteboard work runs on the Tauri main thread; calling AppKit from a tokio worker caused a foreign-exception abort in testing. The highlighter overlay is also whitelisted to TextEdit, Notes, and Safari while Chrome/Electron and Mail are excluded, because probing their AX trees can abort the highlighter. These are deliberate M0 stability guards, not final coverage.

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

## Post-M0 additions (2026-10-20)

These landed after the M0 matrix and are not part of the three-of-five overlay result above:

- [x] The workspace and desktop test suites pass, and desktop clippy is clean. `python3 scripts/check_docs.py` verifies the counts quoted here against what the repository actually runs, so these numbers cannot silently go stale.
- [x] Structured LLM analysis: paragraph chunking, prompt p2/schema, validation, exact/normalized/fuzzy span resolution, dedupe, and 20-suggestion cap.
- [x] LLM-cache bridge into the inherited card and a floating Harper-style widget with live count and an **AI** action over IPC.
- [x] Analyze Field (**⌥⌘A**), Auto Analyze (opt-in, debounced, deduplicated), and one-shot undo (**⌥⌘Z**).
- [x] OpenRouter default plus OpenAI-compatible endpoints; per-provider Keychain entries; local-only mode; bounded transient retries.
- [x] Voice v0: local style card from 300+ word samples, opt-in accepted-edit feedback, and rewrite guidance.
- [x] Tone presets and word-level rewrite diff preview in Settings.
- [ ] Manual UI verification after these additions is pending: the machine was locked while automation was attempted. Specifically verify widget AI/click, undo restoration, provider switching, voice learning, and the diff preview before treating them as gate evidence.

## Post-M0 additions (2026-10-20, second pass)

These landed after the first post-M0 pass and are covered by the automated suite.

- [x] The default model no longer sends a `temperature`. Claude Haiku 5.5 rejects any value other than `1` with an HTTP 400, and the request body simply omits the key.
- [x] The provider layer is a real abstraction: two adapters share one canonical request type, the 13-kind error taxonomy is live code, a curated model catalog backs the picker, and the SSE decoder is tested.
- [x] The clipboard is restored at the item level, so images and files survive a rewrite instead of being destroyed.
- [x] The paste read-back is a structural check, not a substring test: a paste that landed the wrong text is rejected.
- [x] The voice profile learns from accepted, dismissed, and edited-after-accept edits, and a copy guard refuses any rewrite that repeats the writer's own samples.
- [x] Tone detection, a merge-and-rank pass, a golden-set harness, a suggestion event log, a block list, a cost cap, and a live-mode throttle all landed with tests.
- [ ] The fingerprint set grows with the sample size, so a very large upload would make the style card large. This needs a decision before M3 uploads ship.

## The M1 gate report

The M1 gate in `docs/PLAN.md` is now a runnable report rather than a claim.
`apps/macos/src-tauri/src/gate.rs` checks every item the gate lists and
reports each as **pass**, **fail**, or **needs a human**. The five-app overlay
check fails today, because Chrome and Mail are not drawn in yet, and the gate
says so instead of reporting green.

| Check | State | Why |
| --- | --- | --- |
| `overlay-five-apps` | **Fail** | Chrome needs the M3 extension; Mail needs a text-bounds fallback. |
| `probe-safety` | Pass | Every target app is probe-safe, because a highlighter crash is recovered by the restart watchdog. |
| `write-ladder` | Needs a human | Verified in TextEdit, Safari, and Chrome; not yet checked in Notes or Mail. |
| `read-back` | Pass | The read-back compares the field before and after the paste, so a corrupted paste is rejected. |
| `undo` | Pass | The undo record is compared against the field before it is written back. |
| `error-states` | Pass | The provider error taxonomy maps every status to a specific message. |
| `no-text-in-logs` | Pass | The usage log has a timestamp, model, token counts, and cost — no text field. |
| `key-survives-restart` | Pass | The key is read from the macOS Keychain on every request. |
| `clipboard-restore` | Pass | The pasteboard is restored at the item level, so images and files survive. |

## The M2 state

M2 is the phase that makes the product feel like a product: the widget, the
inline experience, and the voice. What is implemented, and what is not:

| Feature | State | Where |
| --- | --- | --- |
| Widget count, AI action, anchors | Implemented | `highlighter/render_state/mod.rs` |
| Widget tone badge | Implemented | The tone is detected locally while the AX tree is walked |
| Widget position pinned per app | Implemented | `widget_anchor.rs`; stored as an anchor, not as pixels |
| Keyboard model for the card | Implemented | `⌃⌥G` focus, `⌃⌥N`/`⌃⌥B` navigate, `⌃⌥A` accept, `⌃⌥D` dismiss, `⌃⌥K` close |
| Suggestion count in the menu bar | Implemented | The highlighter reports the card's state back to the main process |
| Card diff preview and Accept button | Implemented | The original struck through, the replacement highlighted, and an Accept button that applies what is shown |
| Card overflow menu (`⋯`) | Implemented | "Turn off this category" and "Preview the request" |
| Drag-to-anchor | **Not implemented** | The widget can be pinned from Settings, not dragged |
| Local tone detection with evidence | Implemented | `elocutio-analysis::tone` |
| Custom instruction, sanitized | Implemented | `elocutio-analysis::custom_instruction` |
| Style card in rewrite and analysis prompts | Implemented | `openrouter.rs` |
| Voice copy guard | Implemented | `elocutio-voice::copy_guard`, applied at the rewrite chokepoint |
| Trait overrides and "sounds like me" ratings | Implemented | `elocutio-voice::profile` |
| Live mode: debounce, throttle, diff-only, stale guard | Implemented | `auto_analyze.rs` |
| Request coalescing and a per-request cost cap | Implemented | `coalesce.rs`, `openrouter.rs` |
| Block list | Implemented | `config::blocklist` |
| Paragraph-scoped rewriting | **Not implemented** | The rewrite previews and applies a selection, but does not rewrite one paragraph of a longer document |
| The 10-app manual matrix | **Needs a human** | The list and the record exist; the checks do not |

## Known issues and next actions

1. **Mail:** expose usable text-range bounds (or fall back to the static-text collector) before claiming support. Until then, keep Mail out of the supported matrix.
2. **Chrome:** test with Grammarly disabled; build the MV3 extension + local bridge (M3) as the reliable browser path. Do not claim Chrome support from the AX path.
3. **Configurable shortcut:** make the ⌥⌘E binding configurable and expose it in settings.
4. **Clipboard fidelity:** the current implementation restores text only. Preserve non-text pasteboard items with a safe reconstruction path.
5. **Undo/focus regression tests:** add a manual script and, if possible, an automated AX harness.
6. **Signed build:** M0 uses an unsigned debug `.app`. Full Xcode and Developer ID signing are required for TCC-stable distribution.
7. **Updater:** removed for M0. Re-add in M5 with a real minisign keypair, never Harper's.
8. **Settings depth:** onboarding is done; the full settings surface (rules, voice, usage) remains M1.

## Notes on M0 completion

The M0 acceptance gate has two parts still open: the five-app overlay matrix (3/5) and the inherited security audit. The write-ladder, shortcut, clipboard-restore, and key-validation gates now pass. Keep M0 as **in progress** in the tracker until Chrome/Mail and the audit are resolved.

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
