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
- Onboarding checklist for Accessibility, OpenRouter key, and the rewrite
  shortcut; completion is persisted so setup opens only once.
- Floating Harper-style suggestion widget on every supported focused field,
  anchored next to the text or bottom-right when clean; shows the live count,
  opens the inherited card, and exposes the AI analysis action.
- The floating widget has an **AI** action that requests field analysis from the
  main process, so the shortcut is optional.
- Opt-in **Analyze as I write** setting: debounced, deduplicated analysis of the
  focused field, off by default.
- Settings apps list with per-app enablement and auto-enable-new-apps control.
- Optional USD budget warning computed from the local metadata-only usage log.
- Merge policy: local lints win on overlapping spans; LLM suggestions are ordered
  by screen position and validated severity.
- One-shot undo for accepted suggestions via ⌥⌘Z, the tray, or Settings; it
  restores the prior field text only when the applied result is still present.
- Voice v0: local sample analyzer, explainable style card, optional rewrite
  guidance, and a Settings Voice card that stores no raw samples.
- Provider choice: OpenRouter or an OpenAI-compatible base URL/key, with the
  same structured analysis and rewrite paths.
- Bounded retries for transient provider failures (429/5xx/network).
- Opt-in voice learning from accepted edits: local accepted-replacement log,
  concise/contraction traits, and style-card refresh.
- Rewrite tone presets (neutral, confident, friendly, concise, formal) applied to
  the provider prompt.
- Local-only mode blocks analysis, auto-analysis, widget AI, and rewrites while
  keeping local checks active.
- Word-level rewrite previews with inserted/deleted segments in Settings.
- Voice card "What will be sent" preview of the exact style-card guidance.
- LLM-cache bridge: Analyze Field (widget, ⌥⌘A, or tray) publishes validated suggestions;
  the highlighter merges them with local lints in the inherited card.
- Structured LLM analysis pipeline: paragraph chunking, prompt/schema,
  exact/normalized/fuzzy span resolution, validation, dedupe, and
  rejected-edit reporting, with 18 tests.
- Persisted OpenRouter model selection for in-app rewrites and Rewrite
  Selection.
- Local metadata-only usage log at `usage.jsonl` with token and cost totals;
  selected/rewritten text is never recorded.
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
- A real provider abstraction: OpenRouter and OpenAI-compatible adapters share
  one canonical request type, a 13-kind error taxonomy, a curated model
  catalog, and a tested SSE stream decoder.
- The default request no longer sends a `temperature`. Claude Haiku 5.5
  rejects any value other than `1`, so the key is omitted entirely.
- The clipboard is restored at the item level, so images and files survive a
  rewrite instead of being destroyed.
- The paste read-back is a structural check rather than a substring test.
- A ranked, explained merge for local and model suggestions, with every
  dropped suggestion reported.
- Local, deterministic tone detection with the evidence behind each label.
- A golden-set harness with span precision/recall/F0.5, over-edit rate, and the
  plan's regression gate, plus a seed set that ships with the app.
- A suggestion event log that stores the rule, category, and edit size but
  never the text.
- Voice learning from accepted, dismissed, and edited-after-accept edits, with
  a 180-day half-life and down-weighted model-authored text.
- A voice copy guard: a rewrite that repeats the writer's own samples is
  refused, using 8-gram fingerprints so the samples themselves stay discarded.
- Trait overrides, "sounds like me" ratings, and a copy-guard preview.
- A block list: a blocked app is never read and never sent to a provider.
- A per-request cost cap that refuses a request before it is sent.
- Live-mode guards: debounce, per-app throttle, diff-only sending, and a
  stale-result check.
- A tracked app support matrix that a test keeps in step with the code.
- A progress ledger that derives status from evidence, flags unsupported
  claims, and rejects evidence rules that match nothing.
- A custom instruction applied to every rewrite and analysis, sanitized so it
  cannot be used to fake a prompt boundary.
- The voice style card now reaches the inline analysis path, not just rewrites.
- Request coalescing: two callers asking the same question share one request,
  and a burst for the same key is throttled.
- A streaming rewrite path, with the copy guard still applied to the finished
  text.
- A local network log: every host the app has contacted, with the model, the
  token counts, and the cost. It is the proof behind "no hidden network
  calls", and it records no text.
- A request preview: the host, the model, the estimated cost, and the text
  exactly as it will be sent, with the API key redacted.
- The golden set grows from 44 items to 500, with 100 no-error controls, every
  error type covered, and a validator that re-derives every expected offset
  so a wrong one cannot be committed.

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
- The system-wide rewrite previews before it applies, but accept and reject
  are still separate actions in the overlay rather than one control.
- The voice fingerprint set grows with the sample size, so a very large upload
  would make the style card large. This needs a decision before M3.
- Streaming is implemented and tested but not yet wired to a command.
- M0 uses an unsigned debug app; no notarized release or updater yet.

## [0.0.0] - 2026-10-19

- Planning baseline.
