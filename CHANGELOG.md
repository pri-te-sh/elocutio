# Changelog

All notable changes to Elocutio are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims
to follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Product audit, build plan, architecture, feature matrix, decision records,
  prompt library, and voice-learning design.
- Apache-2.0 license, NOTICE with Harper attribution, and open-source
  governance files (contributing, code of conduct, security policy).
- Rust workspace scaffold: `elocutio-core`, `elocutio-analysis`,
  `elocutio-llm`, `elocutio-voice`, `elocutio-store`, `elocutio-bridge`.
- Exact span resolver with UTF-16/grapheme offset tests.
- CI workflow, issue templates, pull request template, and Dependabot.
- Bootstrap, documentation-check, development-signing, and upstream-fetch
  scripts.
- Initial repository scaffold and pinned Harper v2.13.0 upstream lock.
- Brand system v1: twin-quote mark, Ink Indigo palette, Inter typography,
  light/dark lockups, brand guide, and generated platform icons.
- Cross-platform architecture: shared Rust core plus thin macOS, Windows,
  browser, iOS, and Android shells; `elocutio-platform` traits.
- Platform coverage matrix and platform-expansion roadmap.
- Live build-progress dashboard with evidence-derived statuses, milestone and
  acceptance tracking, git activity, filters, and inline status editing
  (`just progress-serve`); static snapshot via `just progress`.
- Validated progress ledger, unit and HTTP tests, and `docs/PROGRESS.md`;
  wired into `just check` and CI.
- M0 macOS shell: rebranded Tauri app, vendored Harper subset under
  `vendor/harper/`, generated app/menu-bar icons, and a minimal Svelte setup UI.
- Keychain-backed OpenRouter key validation and an in-app rewrite path.
- System-wide **Rewrite Selection** action via **⌥⌘E** or the tray menu, using
  a transient/concealed pasteboard string, ⌘V, read-back verification, text
  clipboard restore, and macOS notifications. Verified in TextEdit, Safari, and
  Chrome.
- AX and `NSPasteboard` work is marshalled to the main thread; the M0 overlay is
  whitelisted to TextEdit, Notes, and Safari to avoid foreign-exception aborts
  from Chrome/Mail AX trees.
- Controlled Accessibility test page (`apps/macos/test/overlay.html`) and
  [`docs/M0-VERIFICATION.md`](docs/M0-VERIFICATION.md).
- Brand guide v1.1 with **Soft Indigo `#B8BEFF`** for dark-mode accents and
  optional dark-surface logo, plus dark-mode UI tokens.

### Changed

- The macOS shell now lives in `apps/macos`; the root Cargo workspace excludes
  it and `vendor/harper`.
- Config moved to `~/Library/Application Support/elocutio/`; bundle ID is
  `app.elocutio.desktop`.
- The inherited updater is removed for M0; it returns in M5 with our own
  minisign keypair.
- Product name set to **Elocutio** (ADR-020).
- Build strategy set to fork Harper Desktop (ADR-001).
- License set to Apache-2.0 (ADR-002).

### Fixed

- Dark mode no longer uses Ink Indigo on a dark surface; Soft Indigo accents and
  the white lockup follow the v1.1 contrast rules.
- Suggestion click no longer freezes after bounding AX tree walks and skipping
  unnecessary native-app activation.
- Mail no longer spins the highlighter in an endless activation retry.

### Security

- Documented the key-handling, voice-sample, and local-bridge threat models.
- OpenRouter keys are stored in the macOS Keychain and never written to config
  or logs.
- Rewrite Selection refuses secure input (`IsSecureEventInputEnabled`) and
  secure text fields (`AXSubrole == AXSecureTextField`).

### Known issues

- Chrome and Mail do not show the overlay yet; Chrome needs the M3 extension
  path and Mail needs a text-bounds fallback.
- The Rewrite Selection shortcut is fixed at ⌥⌘E; it is not configurable yet.
- Clipboard restore handles text only; non-text pasteboard items are replaced.
- M0 uses an unsigned debug app; no notarized release or updater yet.

## [0.0.0] - 2026-10-19

- Planning baseline.
