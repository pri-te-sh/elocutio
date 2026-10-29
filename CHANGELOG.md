# Changelog

All notable changes to Elocutio are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims
to follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Elocutio Editorial Atelier UI (`apps/macos/src/App.svelte`)**:
  - Redesigned the macOS studio into a 6-pillar workspace: **Manuscript**, **Analytics**, **Apps**, **Lexicon**, **Voice DNA**, and **Privacy & AI**.
  - **Writing & AI Analytics (`get_writing_analytics`)**: Added an executive writing & AI telemetry dashboard featuring the **Prose Craft Index (0–100)**, **Editorial Time Saved**, **AI Co-Pilot Lift**, **AI Cost Efficiency**, **Hybrid Intelligence Split** (On-Device Harper vs. AI Co-Pilot), **4-Pillar Writing Mastery**, **7-Day Editorial Cadence**, **Active Manuscript Vital Signs**, **AI Model Efficiency Ledger**, and **Signature Refinements**.
  - **AI Provider Presets & Model Slug Discovery**: Added compact dropdown selectors for **OpenRouter**, **Ollama (Local)**, **Ollama Cloud**, **Apple Foundation Models (On-Device)**, **OpenAI Direct**, and **Groq Cloud**, with automatic model slug discovery via `/v1/models` and curated platform fallbacks.
  - **Apple Foundation Models On-Device Bridge (`apple_fm_bridge.rs`)**: Embedded a zero-configuration local OpenAI-compatible HTTP bridge (`127.0.0.1:11535/v1`) powered directly by macOS `FoundationModels.framework` (`SystemLanguageModel.default` and `LanguageModelSession`).
  - **Local SQLite Personalization & Encrypted Credentials (`personalization_db.rs`)**: Added `~/Library/Application Support/elocutio/elocutio.sqlite3` with tables for `dictionary_words`, `editorial_rules`, `edit_events`, `voice_profile`, `app_preferences`, `encrypted_credentials` (AES-256-CBC encrypted API keys with per-installation random salt + PBKDF2-HMAC-SHA256), `ai_provider_configs`, and `ai_token_usage` (prompt/completion tokens, estimated USD cost, latency, and monthly budget tracking).
  - **System-Wide Floating `"elo"` Companion (`elo_overlay.rs`)**: Added a non-activating macOS overlay window with inline error/style underlines, hover-expanded quick actions (`Fix`, `Rewrite`, `Check`), and an inline diff popover.
  - **Dynamic Template Menu-Bar Icon & Streamlined Tray (`tray.rs`)**: Enabled macOS template icon rendering (`icon_as_template(true)`) for automatic dark/light menu-bar contrast and simplified the menu bar dropdown to 4 core actions (`Open Elocutio Studio`, `Pause/Resume Overlay`, `Settings…`, `Quit Elocutio`).
  - **Virtualized Personal Lexicon & Single-Word Guard**: Added a paginated, searchable Lexicon view supporting 1,000+ words with bulk import, inline editing, and strict single-word validation so sentence-level readability lints offer `✦ Tighten Sentence` AI rewrites instead of `+ Lexicon`.
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
- A keyboard model for the suggestion card: ⌃⌥G focuses it, ⌃⌥N and ⌃⌥B
  move between suggestions, ⌃⌥A accepts, ⌃⌥D dismisses, and ⌃⌥K closes.
- The card shows a diff preview and has an Accept button, so a user can see
  what would change before they take it.
- The underlines use the plan's three colours: red for correctness, blue for
  style, grey for advisory.
- The widget collapses to a dot when there is nothing to say, shows the
  detected tone, and can be pinned to a corner per app.
- The menu bar shows the suggestion count and carries the card's actions.
- A settings window that reaches every implemented feature: the writing
  rules, the voice diagnostics, the request preview, the network log, the
  quality report, and the app matrix.

### Changed

- Elocutio can be downloaded: pushing a version tag builds a universal
  (Apple silicon + Intel) app, packages `Elocutio-macOS.dmg`, and publishes it
  to GitHub Releases, signing and notarizing it when Developer ID secrets are
  configured. The website and README link to the latest DMG.
- The overlay's floating widget now appears only while text is selected, as a
  two-button pill (rewrite, suggestions) attached under the selection; a rewrite
  started from it replaces just the selected text. Pausing elo for an app moved
  to the suggestions card.
- Hovering an underlined word in a sentence with several issues opens a
  "Correct the sentence" card that shows the whole corrected sentence as an
  inline diff and applies it in one edit.
- The suggestion card's top bar fits on one row: pause, full editor, and
  settings moved into a "⋯" menu, and the selection pill's icons use the
  brand accents (Ink Indigo, Soft Indigo).

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
- Fuzzy quote matching no longer lands an edit one character off (which
  corrupted text such as "wrld," → "wworld,"), no longer reports real typos as
  ambiguous, and runs in O(n·m) instead of O(n·m²).
- One off-schema suggestion no longer discards every suggestion in a model
  response; trailing prose after the JSON is tolerated; suggestions in
  different paragraphs no longer knock each other out as "overlapping".
- Streamed rewrites keep their token usage, survive invalid UTF-8, and are no
  longer cut off by a 45-second total timeout (an idle timeout applies instead).
- Curly apostrophes (`don’t`) count as contractions in tone and voice analysis;
  the whole Claude 5 family omits `temperature`.
- Restoring the clipboard after a rewrite keeps each type's own bytes, so rich
  text no longer pastes as raw RTF.
- SQLite writes wait out a busy database instead of being silently dropped;
  the schema is created once per launch; `config.json` is written atomically.
- The Studio's local check runs off the async runtime and recovers from a
  linter panic; stale check results and stale lint offsets can no longer
  splice text into the wrong place; overlay card shortcuts are not replayed.
- Settings that fail to load are no longer overwritten with defaults on Save.

### Security

- API keys move from the SQLite `encrypted_credentials` table (whose AES key was
  derived from public values) into the macOS Keychain, as ADR-013 specifies.
  Legacy rows and plaintext `credentials.json` entries are migrated once and
  deleted.
- OpenAI-compatible keys are scoped to the endpoint host, so switching presets
  no longer sends one provider's key to another (or to a local port).
- The Apple Foundation Models bridge refuses browser (`Origin`) and rebound
  (`Host`) requests, caps request size and read time, runs one helper at a
  time with a timeout, and compiles its helper on first use, atomically.
- The cost cap is enforced on every outbound request (live analysis and the
  overlay rewrite skipped it), using the full prompt size.
- Live analysis only runs when the user turns it on, not whenever a key exists.
- Timed-out chat requests are no longer retried (they could bill twice);
  `Retry-After` is honoured; the analysis fallback only re-sends after a
  rejected response format.
- The webview has a strict Content Security Policy; remote base URLs must use
  https.
- "Clear voice profile" also deletes the learned edits stored in SQLite.

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
