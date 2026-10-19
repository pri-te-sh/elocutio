# Build Plan — Elocutio for macOS

**Status:** planning complete (2026-10-19); Harper fork approved; M0 in progress
**Product:** an open-source, bring-your-own-key writing assistant for macOS
**Strategy:** fork/vendor Harper Desktop's Apache-2.0 macOS shell and local rule engine; build the LLM/BYOK judgment layer, UX, and extension as our original contribution
**Product name:** Elocutio (approved 2026-10-19)
**License:** Apache-2.0 (code), CC BY 4.0 (docs)

---

## 1. Vision

> Make the best writing assistant on macOS open, private, and model-agnostic.

Elocutio sits in the menu bar, watches the text field you are already using, and shows small, explainable edits in place. Local rules catch the obvious mistakes instantly; your chosen model handles judgment — clarity, tone, rewrites, translation, drafting — only when needed. Over time it learns **your voice** from the writing you upload and the edits you accept, so rewrites sound like you rather than a generic assistant. You bring an OpenRouter key (or any supported provider, or a local model), and you decide what leaves your Mac.

### Principles

1. **Local-first.** Correctness never depends on the network. The app is usable offline with local rules and a local model.
2. **Judgment on demand.** Do not block typing on a network call. Local rules every keystroke; LLM after a debounce, a shortcut, or an explicit action.
3. **Provider-optional.** OpenRouter is the default because one key reaches many models, not because we depend on it.
4. **Never steal focus.** Suggestions are peripheral; the user stays in flow.
5. **Explain every edit.** A replacement without a reason is a bug.
6. **Visible network.** Every outbound request is logged locally with provider, model, tokens, and cost.
7. **Honest scope.** Publish a tested matrix; no "1M+ apps" claim, no fake plagiarism/AI scores.
8. **Stand on shoulders.** Use permissively licensed prior art (Harper) and contribute back; do not rebuild solved plumbing.
9. **Voice is learned locally.** Your samples and feedback build an explainable, editable profile on your Mac; we do not fine-tune a cloud model on your writing.

### Success criteria for v1.0

| Metric | Target |
| --- | --- |
| Time to first local suggestion | < 50 ms |
| Time to first LLM suggestion (150-word email, default model) | p50 ≤ 1.8 s, p95 ≤ 4.0 s |
| Suggestion accept rate after 4 weeks | ≥ 30% |
| False-positive rate on the golden set | ≤ 8% |
| Crash-free sessions | ≥ 99.5% |
| Idle CPU | ≤ 0.5% |
| Resident memory | ≤ 220 MB |
| Onboarding completion (permissions + key validated) | ≥ 65% |
| Default cost per 1,000 email reviews | ≤ $0.30 |
| "Sounds like me" rating after 3 samples | ≥ 4.0 / 5 |
| Voice-copy rate (8-gram overlap with samples) | 0 |
| Voice profile adoption (opt-in, active users) | ≥ 40% |
| Network requests not initiated by a configured provider | 0 |
| Tier A apps in the tested matrix | ≥ 12 |

Non-goals for v1.0: plagiarism parity, AI-detector parity, enterprise admin/SSO/SCIM/DLP, real-time collaboration, cloud sync, cloud fine-tuning of a model on user writing (voice uses a local style card + retrieval), and impersonating another person. **Windows, iOS, Android, and Safari are planned platform expansions after the macOS release, not v1 deliverables.**

---

## 2. Build strategy: fork Harper, add the judgment layer

### 2.1 The decision

The hardest and least differentiated part of this product is the macOS plumbing: reading text from arbitrary apps through Accessibility, drawing underlines over foreign windows, tracking window movement, handling per-app enablement, and replacing text safely. **Harper Desktop already does this, is Apache-2.0, and is in the open.**

The differentiated part is the judgment layer: provider abstraction, prompt design, structured-edit validation, offset resolution, tone/rewrite/compose/translate/humanize, cost transparency, privacy controls, and the browser extension. That is what we build.

| Option | What it means | Effort to MVP | Effort to v1.0 | Verdict |
| --- | --- | --- | --- | --- |
| **A. Fork/vendor Harper Desktop** | Reuse AX broker, highlighter, tray, updater, local rules; build LLM layer + voice + UX | 4–6 weeks | 14–19 weeks | **Recommended** |
| **B. Native Swift + harper-core FFI** | Port/reimplement AX/highlighter in Swift; reuse local engine | 7–10 weeks | 16–22 weeks | Fallback if Tauri shell fails |
| **C. Clean-room native Swift + own rules** | Build everything; weakest local grammar | 10–14 weeks | 20–28 weeks | Not recommended |

### 2.2 What we reuse from Harper (Apache-2.0)

| Component | Path (Harper repo) | Use |
| --- | --- | --- |
| Local grammar engine | `harper-core` | <10 ms English checks: spelling, grammar, punctuation, style, dialect, dictionary |
| macOS Accessibility broker | `harper-desktop/src-tauri/src/mac_broker/` | Focused element, text extraction, text-range bounds, app catalog, window stability, activation handling |
| Highlighter | `harper-desktop/src-tauri/src/highlighter/`, `highlighter_service/` | Overlay windows, underlines, hit-testing, per-app rendering |
| OS broker | `os_broker.rs`, `windows_broker/` | Platform abstraction; Windows path later |
| Tray and config | `tray.rs`, `config/` | Menu-bar app, per-app enablement, settings persistence |
| Updater/autostart | `desktop_updater.rs`, Tauri plugins | Signed updates, launch at login |
| Tauri shell | `harper-desktop/src-tauri/` | App lifecycle, IPC, bundling |

### 2.3 What we build (our original work)

- `elocutio-llm` crate: provider adapters, canonical request/response, streaming, retries, capability descriptors, cost estimation.
- Prompt library and JSON schemas for every writing task.
- Structured-edit validation and offset resolution (the quality core).
- Local + LLM suggestion merge, ranking, dedupe, caching.
- Grammarly-style widget, suggestion cards, assistant panel, onboarding, settings.
- Keychain, usage log, network log, budget caps, privacy controls.
- Browser bridge and Chrome/Edge extension.
- Style guide, brand voice, glossary, snippets, provenance, history.
- New brand/design system and documentation.

### 2.4 Fork conditions

1. Pin to a tagged Harper release (start at **v2.13.0**, 16 Oct 2026); keep `upstream` remote; document cherry-pick/sync policy in `UPSTREAM.md`.
2. Retain Apache-2.0 `LICENSE` and add a `NOTICE` with Harper's copyright and our changes.
3. Remove all Harper trademarks, product names, icons, bundle IDs, URLs, and config paths. Replace with Elocutio equivalents.
4. Do not imply endorsement by Automattic.
5. Add a clear "Built on Harper" attribution in README and About.
6. Run a security review of the inherited code (network surface, IPC, updater, permissions).
7. If the M0 spike shows the Tauri/highlighter shell cannot deliver non-activating, correctly aligned UI in our Tier A apps, stop and switch to Option B before investing in the LLM layer.

---

## 3. Scope and phase gates

| Gate | Audience | What must be true |
| --- | --- | --- |
| **M0 — Fork & spike** | Internal | Rebranded app builds and runs; overlay works in 5 apps; OpenRouter key validates; a minimal LLM rewrite replaces a selection |
| **M1 — LLM MVP** | Early users | Local + LLM suggestions in the focused field; suggestion card; accept/dismiss; tone detection; onboarding; key in Keychain; usage log |
| **M2 — Inline UX + voice v0** | Private beta | Widget, underlines, tone rewrite, paragraph rewrite with diff, cache, secure-field/blocklist polish, 10-app matrix; voice profile from feedback |
| **M3 — Everywhere + voice uploads** | Public beta | Chrome/Edge extension, assistant panel, compose/reply/summarize, multi-provider; sample uploads |
| **M4 — Personal & trust + voice v2** | Release candidate | Style guide, brand voice, glossary, snippets, provenance, local history, translation/humanizer, 24-language support; per-context voices |
| **M5 — Launch** | v1.0 | Notarized DMG, signed updates, Homebrew, docs, support matrix, security review |

### Scope by phase

| Capability | M1 | M2 | M3 | M4 | M5 |
| --- | --- | --- | --- | --- | --- |
| Local correctness (Harper) | ● | ● | ● | ● | ● |
| LLM proofread (grammar/clarity/conciseness) | ● | ● | ● | ● | ● |
| Tone detection | ● | ● | ● | ● | ● |
| Tone rewrite + custom instruction | ● | ● | ● | ● | ● |
| Voice profile from feedback + manual overrides | | ● | ● | ● | ● |
| Upload writing samples (txt/md/docx) | | | ● | ● | ● |
| Per-context + multiple voice profiles | | | | ● | ● |
| Voice preview + "sounds like me" rating | | ● | ● | ● | ● |
| Inline underlines for LLM suggestions | | ● | ● | ● | ● |
| Floating widget + per-app anchors | | ● | ● | ● | ● |
| Paragraph rewrite + diff | | ● | ● | ● | ● |
| Browser extension (Chrome/Edge) | | | ● | ● | ● |
| Assistant panel + compose/reply/summarize | | | ● | ● | ● |
| Google Docs support | — | — | — | — | — |
| Multi-provider (Anthropic, Gemini, Ollama) | ○ | ○ | ● | ● | ● |
| Style guide / brand voice / glossary | | | | ● | ● |
| Snippets / dictionary UI | ○ | ● | ● | ● | ● |
| Provenance / Authorship-style log | | | | ● | ● |
| History + local analytics | | | | ● | ● |
| Translation / humanizer / 24 languages | ○ | ○ | ● | ● | ● |
| Apple on-device model | | | | ○ | ● |
| Citations / web similarity | | | | | ○ |
| Safari extension | | | | | ○ |
| Plagiarism corpus / AI detector | — | — | — | — | — |

● planned, ○ stretch, — out of scope.

### The MVP in one sentence

**Put the cursor in any text field, press a shortcut, see the few things worth fixing — with local rules answering instantly and your OpenRouter model handling judgment — click one, and the text is corrected.**

---

## 4. User experience

### 4.1 First run

1. **Welcome.** "Elocutio proofreads where you type. Local rules run on your Mac; your chosen model handles the hard parts. Nothing leaves your Mac unless you configure a provider."
2. **Choose engine.**
   - *Local only*: Harper rules, no key, no network.
   - *OpenRouter* (recommended): paste key → validate → show credit/limit.
   - *Other provider*: base URL + key (OpenAI-compatible).
3. **Pick a model.** Curated cards: "Fast & cheap (default: Claude Haiku 5.5)", "Balanced", "Best quality"; browse all. Show price per 1M tokens and context.
4. **Grant Accessibility.** Explain exactly why and what is read. Button opens Settings; poll and advance.
5. **Optional:** launch at login, screen-recording OCR fallback, crash reports (off).
6. **Optional: teach your voice.** Add 3 samples or skip. Explain that samples stay encrypted on the Mac and that the style card is all that is sent. This step is skippable and can be done later in Settings → Voice.
7. **Try it.** A playground with three deliberate errors; user presses the shortcut and accepts a fix.
8. **Done.** Menu bar icon; a short card shows the shortcut and the per-app block list.

Failures handled inline: invalid key, no credits, rate limit, model unavailable, offline, unsupported structured output.

### 4.2 Core loop

```
User focuses a text field
        │
        ▼
Harper AX broker detects focus/value change (debounced)
        │
        ▼
Text captured: app, element, selection, full/paragraph text
        │
        ├──────────────► Harper local rules (<10 ms)
        │                     │
        │                     ▼
        │               red/blue/gray underlines immediately
        │
        ▼
LLM trigger: shortcut, debounce (700 ms), or explicit action
        │
        ▼
Chunk (600–1,200 tokens) → prompt → provider (JSON schema)
        │
        ▼
Validate → resolve spans → dedupe → merge with local lints
        │
        ▼
Widget: count + tone  ──►  Suggestion card  ──►  Accept/Dismiss
                                                    │
                                                    ▼
                                         pasteboard+⌘V / AX fast path
                                                    │
                                                    ▼
                                            Read-back verify + undo
```

### 4.3 Surfaces

#### Menu bar

- Icon states: idle, local-only, analyzing, suggestions available, error, paused.
- Menu: "3 suggestions", tone, "Analyze selection" (`⌃⌥G`), "Rewrite…" (`⌃⌥R`), "Open assistant" (`⌃⌥Space`), pause 1 hour, turn off for app, settings, usage, about, quit.

#### Floating widget

```
┌─────────────────────┐
│  ✦ 3 suggestions  🙂 │
└─────────────────────┘
```

- Appears after the first suggestion; collapses to a dot when idle.
- Draggable to anchor points; position saved per app/site.
- Click opens the card stack; never activates the host app.
- Keyboard: `⌃⌥G` focuses; arrows navigate; `Return` accepts; `Esc` closes.

#### Suggestion card

```
┌───────────────────────────────────────────┐
│  Grammar                            ✕     │
│  "tehre" → "there"                        │
│  Spelling: 'tehre' is not a word.         │
│                                           │
│  [ Accept ]  [ Dismiss ]  [ ⋯ ]           │
└───────────────────────────────────────────┘
```

- One card at a time; stack for multiple.
- Diff preview: original struck through, replacement highlighted.
- `⋯`: Explain more, Ignore this rule, Add to dictionary, Turn off category, Preview request.

#### Assistant panel

- Slides from the right edge; 360–420 pt; resizable; tabs Suggestions / Chat / Rewrite.
- Context chips: app, selection size, document size; remove before sending.
- Quick actions: Improve, Shorten, Formalize, Summarize, Reply, Translate, Humanize.
- Streaming; stop; copy/insert/replace; never steals focus until clicked.

#### Settings

| Tab | Contents |
| --- | --- |
| General | Launch at login, menu bar, language, appearance, update channel |
| Providers | Add/edit/test/delete, default, fallback chain, key storage |
| Models | Default per task, catalog refresh, budget caps, escalation policy |
| Writing | Local rules, LLM categories, dialect, tone, aggressiveness, preferences in natural language |
| Voice | Profiles, samples, learned traits, strength, per-context mapping, preview, reset |
| Apps | Per-app mode (off / local / local+LLM / live), block list, widget position |
| Shortcuts | Global shortcuts, conflict detection |
| Privacy | Local-only mode, redaction, preview request, history retention, network log |
| Usage | Tokens, cost by provider/model/day, budget, export CSV |
| Advanced | Base URLs, proxy, experimental, diagnostics |
| About | Version, updates, licenses, source, "Built on Harper" |

#### Voice

A dedicated **Voice** tab and a first-run "Teach Elocutio your voice" card:

- **Profile card:** name, context, sample word count, confidence, one-line summary.
- **Two switches:** "Learn from my edits" (off by default) and "Use my voice" (off by default); they are independent.
- **Voice strength:** Off / Light / Balanced / Strong (default Balanced for rewrites, Light for clarity).
- **Add samples:** drag `.txt`/`.md`/`.docx`, choose files, or paste; review extracted text and redactions before saving.
- **Review what Elocutio learned:** trait groups (rhythm, diction, structure, punctuation, tone) with values, confidence, evidence, and overrides.
- **Learned from your edits:** accepted/dismissed counts and inferred preferences ("you remove 'just' often"), each undoable.
- **Preview:** generate a short rewrite in your voice, rate "sounds like me," adjust strength.
- **What will be sent:** exact style card + exemplars; "style card only" mode.
- **Profiles:** Personal, Work, Brand; per-context (email/chat/documents/social/support) and per-app mapping.
- **Data:** freeze profile, reset learned values, delete samples/profile, export/import JSON.

In-flow: rewrite menu has a "Use my voice" toggle; the assistant panel has a "Match my voice" chip; generated text has a "This doesn't sound like me" action.

Full design: [VOICE-LEARNING.md](VOICE-LEARNING.md).

### 4.4 Shortcuts (defaults)

| Shortcut | Action |
| --- | --- |
| `⌃⌥G` | Analyze selection or focused field |
| `⌃⌥R` | Rewrite selection (tone menu) |
| `⌃⌥Space` | Assistant panel |
| `⌃⌥A` | Accept top suggestion |
| `⌃⌥D` | Dismiss top suggestion |
| `⌃⌥P` | Pause/resume |
| `⌃⌥L` | Toggle live LLM mode for current app |
| `⌃⌥V` | Toggle "Use my voice" for the current task |
| `Esc` | Close card/panel |

Use Carbon `RegisterEventHotKey` (or Tauri global-shortcut) to avoid Input Monitoring where possible.

### 4.5 Live behavior

- Local rules: every keystroke, <10 ms, debounced rendering.
- LLM: opt-in per app; 700 ms debounce; diff-only (changed paragraph + one neighbor); cancel on keystroke; stale-hash guard; budget cap; never send secure fields.
- Long text: live analyzes the caret paragraph; "Review all" chunks the field.
- Cost estimate before analyzing >5,000 chars.

### 4.6 App support matrix

| Tier | Apps |
| --- | --- |
| A | TextEdit, Notes, Mail, Messages, Safari (textareas/contenteditable), Chrome (textareas/contenteditable), Pages, Microsoft Word, Slack, Obsidian, Gmail (extension) |
| B | Notion desktop, Linear, Jira, Confluence, VS Code, Excel, PowerPoint, Outlook, Teams, Figma comments |
| Unsupported | Google Docs (canvas; no DOM/AX text), password fields, secure input, Terminal/iTerm, remote desktops, games, apps that block Accessibility, Java Swing |

**Google Docs is explicitly unsupported in v1.** Docs switched to canvas rendering in 2021; neither Accessibility nor a content script reliably sees or decorates the text. Users get a clear message and a "copy into Elocutio's editor" path. Revisit with a Docs Add-on or API integration post-1.0.

Ship the matrix in Settings → Apps with per-app status and a "Report a problem" link.

### 4.7 Error states

| State | UI | Retry |
| --- | --- | --- |
| Invalid key (401) | "OpenRouter rejected this key." Fix in Settings | On change |
| No credits (402) | "Your balance is empty." Link to credits | Manual |
| Rate limit (429) | "Rate-limited. Retrying in 8 s…" Cancel | Backoff, max 2 |
| Unsupported model/params (400/422) | "This model can't return structured edits. Try [recommended]." | One fallback |
| Timeout | "Taking longer than usual." Wait/cancel | 30 s first byte |
| Offline | "Offline. Local rules still work." | On reconnect |
| Malformed JSON | One silent repair; then diagnostics | Max 1 |
| No Accessibility | Onboarding card with deep link; playground still works | Poll |

---

## 5. Architecture summary

Full design: [`ARCHITECTURE.md`](ARCHITECTURE.md).

### 5.1 Process model

```text
┌──────────────────────────── Tauri main process (Rust) ────────────────────────────┐
│ tray/menu · app lifecycle · updater · autostart · config                          │
│                                                                                   │
│  ┌────────────────────┐   ┌──────────────────────┐   ┌─────────────────────────┐  │
│  │ Provider manager   │   │ LLM orchestrator     │   │ Browser bridge (WS)     │  │
│  │ Keychain + HTTP    │◄─►│ chunk · prompt ·     │◄─►│ pairing · protocol      │  │
│  │ OpenRouter/OpenAI/ │   │ validate · resolve · │   └─────────────────────────┘  │
│  │ Anthropic/Gemini/  │   │ dedupe · cache       │                                │
│  │ Ollama             │   └──────────┬───────────┘                                │
│  └────────────────────┘              │                                            │
│                                      │ IPC (stdin/stdout, JSON)                   │
│  ┌────────────────────┐              │                                            │
│  │ SQLite: usage,     │              │                                            │
│  │ config, cache,     │              │                                            │
│  │ style, snippets    │              │                                            │
│  └────────────────────┘              │                                            │
└──────────────────────────────────────┼────────────────────────────────────────────┘
                                       │
┌──────────────────────────────────────▼────────────────────────────────────────────┐
│ Highlighter subprocess (Rust + egui/winit)                                        │
│ AX broker: focused element · text · selection · range bounds · app catalog        │
│ Local rules (harper-core) every keystroke                                         │
│ Overlay windows: underlines · widget · suggestion cards · hit-testing             │
│ Text injection: AX write, pasteboard fallback, read-back verification             │
└───────────────────────────────────────────────────────────────────────────────────┘
```

- **Main process** owns secrets, network, provider config, cost accounting, and the bridge. It never draws inside other apps.
- **Highlighter subprocess** owns Accessibility, geometry, local rules, and text injection. It has no provider keys and no network access beyond the main process IPC. This is a security boundary.
- **Tauri webviews** render onboarding, settings, usage, and the assistant panel. They talk to the main process through Tauri commands.

### 5.2 Module map

| Module | Status | Responsibility |
| --- | --- | --- |
| `harper-core` | vendor | Local grammar engine (English) |
| `harper-desktop` (forked into `apps/macos`) | fork | Tauri shell, tray, config, highlighter, AX broker, updater |
| `elocutio-llm` | new | Canonical provider types, adapters, streaming, retries, costs |
| `elocutio-analysis` | new | Chunking, prompts, JSON schema, validation, offset resolver, dedupe, merge |
| `elocutio-voice` | new | Sample ingestion, feature extraction, style card, preference learning, per-context profiles |
| `elocutio-store` | new | SQLite schema/migrations, repositories, keychain wrapper |
| `elocutio-bridge` | new | Local WebSocket server, pairing, extension protocol |
| `elocutio-platform` | new | Shared traits for capture, injection, overlay, and permissions; implemented per platform |
| `apps/macos` | fork + new | macOS Tauri shell, AX broker, highlighter, tray, updater |
| `apps/windows` | later | Windows Tauri shell, UIA broker, overlay |
| `apps/ios` | later | SwiftUI app + keyboard/share extensions |
| `apps/android` | later | Compose app + IME/AccessibilityService |
| `extensions/chrome` | M3 | MV3 content script, overlay, popup, background |
| `elocutio-cli` | later | Headless lint/rewrite for scripts and CI |

### 5.3 Stack

- Rust 2024 edition, Tauri 2, `tokio`, `reqwest` (rustls), `serde`, `rusqlite` (bundled), `security-framework`, `tracing`.
- Svelte 5 + TypeScript + Vite for webviews.
- `harper-core` 2.13.x (English).
- Tauri updater (minisign) instead of Sparkle; Tauri autostart; Tauri bundler.
- Tests: `cargo test`, `insta` snapshots, `axum` fake provider, Playwright for the extension, `cargo clippy`, `rustfmt`, `eslint`/`prettier`.
- CI: GitHub Actions macOS runner for build/test; release workflow for signing/notarization.

### 5.4 Provider defaults

| Task | Default | Escalation |
| --- | --- | --- |
| Proofread / clarity | `anthropic/claude-haiku-5.5` | `claude-sonnet-5` |
| Tone detection | `claude-haiku-5.5` | — |
| Rewrite / compose / translate | `claude-haiku-5.5` | `claude-sonnet-5` → `gpt-5.6-sol` |
| Long document | `claude-haiku-5.5` (chunked) | `claude-sonnet-5` |
| Budget mode | `deepseek/deepseek-v4.1-flash` | — |
| Local | Apple Foundation Model (macOS 26) or Ollama | — |

Default settings: `temperature: 0`, `reasoning.effort: "none"` where supported, `response_format: json_schema`, `provider.require_parameters: true`, `provider.data_collection: "deny"`, optional `zdr: true`.

### 5.5 Suggestion contract

The model returns **verbatim quotes**, never offsets:

```json
{
  "suggestions": [
    {
      "paragraph_id": "p3",
      "original": "tehre",
      "replacement": "there",
      "category": "spelling",
      "severity": "high",
      "explanation": "Misspelling of 'there'."
    }
  ]
}
```

The client:

1. Validates the schema.
2. Locates the paragraph and does an exact match; then normalized (NFC, whitespace, smart quotes); then fuzzy (≥0.85 similarity).
3. Drops ambiguous/unresolved edits.
4. Stores both UTF-16 and grapheme offsets.
5. Dedupes overlaps, preferring severity → resolution quality → interior spans.
6. Applies edits right-to-left and reads back.

No model-provided offsets, no self-reported confidence.

### 5.6 Privacy boundary

- Keys live in the main process Keychain; the highlighter never sees them.
- The highlighter sends text to the main process only when analysis is triggered; the main process logs provider/model/tokens/cost, not text.
- Local-only mode: no network calls at all; Harper + optional local model.
- Per-app rules, secure-field exclusion, redaction, request preview, network log.
- No telemetry; opt-in crash reports only.

### 5.7 Non-negotiable macOS constraints (verified)

- **App Sandbox cannot read other apps' text through Accessibility.** The app is non-sandboxed, Developer ID-signed, and notarized; no Mac App Store. A future Safari extension must live inside the same notarized app.
- **Focus is sacred.** Every overlay window is a non-activating panel with `canBecomeKey = false`; the card is click-only (no text input). A single activation call moves the caret and breaks the subsequent paste.
- **The write path is a ladder.** Re-read immediately before injecting. AX fast path only when `AXValue` is settable, the role is a native text role, the app is not Chromium-family, and the field is short. Otherwise pasteboard + ⌘V with transient/concealed tags, restore-by-item, and read-back verification; abort on mismatch. Chromium ignores AX `AXValue`/`AXSelectedText` writes (confirmed in Chromium source).
- **Google Docs is unsupported in v1** (canvas rendering). Show a clear fallback; do not promise it.
- **OCR is opt-in**, never the default; it requires Screen Recording and periodic re-consent.
- **Underlines** use a click-through window at `CGWindowLevel(1500)` (`assistiveTechHigh`), one per screen, hidden on app/space switches, with implicit animations disabled.
- **TCC identity follows the code signature.** Use a stable self-signed development certificate and fixed bundle ID from day one.

---

## 6. Repository layout

Fork the Harper monorepo, then add our crates/apps. Keep upstream sync manageable.

```
elocutio/
├── README.md
├── LICENSE                      # Apache-2.0
├── NOTICE                       # Harper attribution + our changes
├── UPSTREAM.md                  # fork base, sync policy
├── CONTRIBUTING.md
├── SECURITY.md
├── docs/
│   ├── AUDIT.md
│   ├── PLAN.md
│   ├── ARCHITECTURE.md
│   ├── FEATURE-MATRIX.md
│   ├── DECISIONS.md
│   ├── PROMPTS.md
│   ├── VOICE-LEARNING.md
│   ├── BRAND.md
│   ├── PRIVACY.md
│   ├── TESTING.md
│   └── MAINTAINING.md
├── harper-core/                 # vendored (English rules)
├── harper-*/                    # vendored dependencies
├── apps/
│   ├── macos/                    # forked from harper-desktop
│   │   ├── src-tauri/
│   │   │   ├── src/
│   │   │   │   ├── mac_broker/   # inherited; audited
│   │   │   │   ├── highlighter/  # inherited; extended
│   │   │   │   ├── tray.rs
│   │   │   │   ├── config/
│   │   │   │   └── llm/          # new: orchestration, commands
│   │   │   └── tauri.conf.json   # rebranded
│   │   └── src/                  # Svelte UI (new design)
│   ├── windows/                  # post-v1: Tauri + UIA
│   ├── ios/                      # planned: SwiftUI + keyboard extension
│   └── android/                  # planned: Compose + IME/accessibility
├── crates/
│   ├── elocutio-core/
│   ├── elocutio-analysis/
│   ├── elocutio-llm/
│   ├── elocutio-voice/           # samples, traits, style card, feedback learning
│   ├── elocutio-store/
│   ├── elocutio-bridge/
│   └── elocutio-platform/        # shared platform traits (M0/M1)
├── extensions/
│   ├── chrome/
│   ├── safari/                   # later
│   └── firefox/                  # later
├── tools/
│   ├── fake-provider/
│   └── eval/
└── scripts/
    ├── dev.sh
    ├── sign.sh
    ├── notarize.sh
    └── release.sh
```

If vendoring the entire monorepo proves unwieldy, extract `apps/macos` + the required `harper-*` crates into a smaller workspace and pin `harper-core` by git tag. Decide in M0 after measuring build times.

---

## 7. Milestones (Option A)

Estimates assume one experienced Rust/TypeScript engineer full-time; add 30–50% part-time.

### M0 — Fork, rebrand, spike (5–7 days)

- Fork Harper at v2.13.0; add `upstream`; write `UPSTREAM.md`.
- Rebrand: product name, bundle ID, icons, tray, config paths, updater keys/endpoint, About.
- Build/run on macOS 26; verify AX overlay in TextEdit, Notes, Mail, Safari, Chrome.
- Add `elocutio-llm` skeleton: OpenRouter client, Keychain, model catalog, key validation.
- Add a minimal "Rewrite selection with AI" action in the highlighter to prove the end-to-end path.
- Decide IPC ownership: LLM in main process, rendering in highlighter; document the protocol.
- Security audit of inherited network/IPC/updater code.

**Acceptance:** rebranded app builds; overlay works in 5 apps; key validates; a rewrite replaces the selection correctly using the write ladder (pasteboard + ⌘V primary, AX fast path in AppKit); the card never steals focus; no Harper branding remains.

### M1 — LLM MVP (10–14 days)

- Provider abstraction + OpenRouter + OpenAI-compatible; streaming; errors; retries.
- Chunking, prompt v1, JSON schema, validation, offset resolver, dedupe.
- Merge local + LLM suggestions; ranking.
- Suggestion card UI (accept/dismiss/explain); inherited local underlines; LLM suggestions in the card.
- Onboarding: local-only vs provider, key, model, permissions, playground.
- Settings: providers/models, per-app enable, privacy, usage.
- Keychain, cost log, budget warning.
- Record accept/dismiss/edit-after-accept events locally (only when voice learning is enabled) so voice v0 can learn from them in M2.
- Golden-set harness v0.

**Acceptance:** in 5 apps (including a Chromium-based app), a shortcut produces suggestions; accept edits the correct span via pasteboard or the AppKit AX fast path; read-back verification passes; undo works; invalid/offline/402 states are clear; zero text in logs; key survives restart.

### M2 — Inline UX + Voice v0 (13–18 days)

- Grammarly-style widget: count, tone, anchors, per-app position.
- Live LLM mode: debounce, diff-only, cancel, stale guard, throttles.
- Tone detection + tone rewrite presets + custom instruction.
- Paragraph rewrite with word-level diff and accept/reject.
- Cache + request coalescing + cost caps.
- Secure-field and blocklist hardening.
- Tier A matrix: 10 apps, light/dark, multi-monitor, Spaces, full-screen.
- **Voice v0 (opt-in):** build a profile from accepted/dismissed/edited suggestions; manual trait overrides; style card; apply to rewrites; "sounds like me" rating; Voice settings tab.

**Acceptance:** widget never activates the host app; underlines align within ±2 pt; live p95 < 4 s; position persists; no request storm. Voice: after 5 accepted/edited suggestions, the profile shows at least 3 learned traits with confidence and can be applied to a rewrite; `elocutio-voice` copy guard passes.

### M3 — Everywhere + Voice uploads (19–25 days)

- Chrome/Edge MV3 extension: content script, shadow-DOM overlay, popup, background.
- Local bridge: WebSocket, pairing token, Origin allowlist, reconnect, backpressure.
- Gmail, Notion, Slack web, Linear, and generic textarea/contenteditable adapters.
- Google Docs: explicit unsupported message and "copy into Elocutio" path (canvas has no reliable DOM/AX text).
- Assistant panel: chat with context chips, streaming, insert/replace.
- Compose/reply/summarize.
- Providers: Anthropic, Gemini, Ollama, custom OpenAI-compatible.
- Provider routing UI: per-task default, fallback, budget.
- **Voice v1:** upload `.txt`/`.md`/`.docx` or paste; parse locally; quality gates; extract traits; select exemplars; preview; delete; encrypted sample storage.

**Acceptance:** extension works on Gmail, Notion, Slack web, Linear, and a plain textarea; Google Docs shows a clear unsupported state; pairing is one-time and rejects unpaired origins; switching provider takes <3 clicks; reply inserts at the caret. Voice: 3 uploaded samples produce a style card with confidence and a preview rewrite; deleting a sample removes its exemplars and stats.

### M4 — Personal & trust + Voice v2 (14–20 days)

- Style guide YAML import/export, regex/term rules, severity, examples.
- Brand voice from 3–10 sample documents (few-shot profile).
- Glossary, snippets, dictionary UI.
- Provenance log (typing/paste hashes/AI edits) + export.
- Local history/analytics (opt-in).
- AI-phrase linter (style feedback, not a verdict).
- Translation + humanizer presets; 24-language support via LLM.
- Accessibility and localization pass.
- **Voice v2:** per-context and multiple profiles; exemplar retrieval; drift detection; export/import; PDF support; brand voice; freeze profile.

**Acceptance:** a shared style guide survives restart and is enforced; provenance exports; no AI-detector score; local-only mode works offline. Voice: email/chat/document contexts select the right profile; a drifted profile prompts an update; export/import round-trips; PDF ingestion works or fails with a clear message.

### M5 — Launch (8–12 days)

- Developer ID signing, hardened runtime, notarization, DMG.
- Tauri updater: signing keys, appcast endpoint, stable/beta channels.
- Homebrew cask, GitHub Releases, checksums.
- Docs, website, support matrix, privacy/security pages.
- Opt-in crash reporting, diagnostics export.
- Security review: key handling, bridge auth, IPC, dependencies, update verification.
- Release checklist and rollback.

**Acceptance:** clean Mac installs from DMG, grants permissions, onboards, and updates via the app; Gatekeeper passes; `brew install --cask` works.

### Total (Option A)

| Phase | Days | Calendar (full-time) |
| --- | --- | --- |
| M0 | 5–7 | week 1 |
| M1 | 10–14 | weeks 2–3 |
| M2 | 13–18 | weeks 4–7 |
| M3 | 19–25 | weeks 8–12 |
| M4 | 14–20 | weeks 13–16 |
| M5 | 8–12 | weeks 17–19 |
| **Total** | **69–96** | **~14–19 weeks** |

MVP (through M1) ~3–5 weeks of engineering; plan for 4–6 with buffer. Public beta (through M3) ~10–13 weeks. v1.0 ~14–19 weeks. Voice learning adds ~11–14 engineering days across M2–M4. Option B adds 6–10 weeks.

### Platform expansion (after v1)

These are separate milestones with their own acceptance criteria; they are not part of the macOS v1 date.

| Order | Platform | Reuse | New work | Main risk |
| --- | --- | --- | --- | --- |
| 1 | **Chrome/Edge extension** | Protocol, providers, analysis, voice | MV3 UI, adapters, pairing | Web editor quirks |
| 2 | **Windows** | All shared crates, protocol, voice | UIA broker, overlay, installer, signing | UIA inconsistency; antivirus/signing |
| 3 | **iOS** | Core via UniFFI, voice, providers | Keyboard/share extensions, App Store | No cross-app access; keyboard UX |
| 4 | **Android** | Core via UniFFI/JNI, voice, providers | IME, optional AccessibilityService, Play policy | Permissions/OEM variance |
| 5 | **Safari/Firefox** | Extension code | WebExtension adaptation, Safari packaging | Safari distribution |

Rules: do not duplicate the analysis engine; do not fork the provider layer; keep voice profiles portable; ship only after a tested app matrix.

---

## 8. Testing

### Unit

- Offset resolver: exact/normalized/fuzzy, Unicode, emoji, CRLF, combining marks, RTL, CJK, ambiguous duplicates; property-based tests.
- Chunker: boundaries, overlap, max size, grapheme clusters.
- Schema validation: missing/extra fields, empty replacement, no-op, overlapping spans.
- Provider adapters: recorded fixtures for success/stream/401/402/429/400/malformed SSE/JSON repair; capability checks.
- Cost estimator: per-model pricing, cache tokens, long-context overrides.
- Keychain: store/read/delete, no secrets in logs.
- Style rules: YAML parse, regex safety, precedence.
- Local+LLM merge: overlapping edits, severity ranking, dedupe.
- Voice feature extraction: rhythm/diction/structure/punctuation/tone on golden samples; confidence; language detection.
- Voice style card: deterministic summary, overrides, decay, per-context selection.
- Voice ingestion: txt/md/docx parsing, dedupe, quality gate, quoted-reply stripping, size limits.
- Voice copy guard: 8-gram overlap detection and rejection.
- Voice feedback: weights, preference inference thresholds, AI-text down-weighting, drift detection.

### Integration

- `tools/fake-provider` (axum): deterministic responses, slow/429/malformed/partial-stream scenarios.
- End-to-end: capture fixture text → analyze → accept → verify final text.
- Bridge: pairing, auth rejection, reconnect, framing, backpressure.
- Updater: signature verification, manifest parsing.
- Highlighter: golden screenshots for alignment in 5 apps (manual/CI-assisted).

### Manual matrix (every release)

- Apps: 12 Tier A + 6 Tier B.
- OS: current + previous major; light/dark; increased contrast; reduced motion.
- Displays: Retina/non-Retina, two monitors, scaling; Spaces; full-screen; Stage Manager.
- Text: empty, 1 char, 10k chars, emoji, RTL, CJK, code blocks, markdown, rich text with links.
- Permissions: fresh install, revoked mid-session, upgrade, second user.
- Network: offline, slow, captive portal, proxy, VPN.
- Security: secure field, password manager, terminal, screen sharing.
- Voice: upload 3 samples in each context; check extracted text, traits, preview, deletion; verify style-card-only mode sends no excerpts.

### Quality evaluation

- Golden set: 200–500 items, stratified by error type, length, source, difficulty, no-error controls.
- Metrics: span precision/recall/F0.5, exact acceptance, ERRANT M2 F0.5, GLEU, meaning preservation, over-edit rate, schema failure, resolution rate, latency, cost.
- Regression gate: F0.5 drop >2 points, resolution drop >5 points, or cost increase >20% fails CI.
- Human review during beta: 50 suggestions/week labelled accept/edit/reject.
- Voice evaluation: held-out trait similarity, "sounds like me" rating, blind A/B, copy guard, meaning preservation, feedback-loop drift.

### Performance

- `cargo bench` for resolver/chunker; `tracing` spans for capture→request→render.
- Instruments/Activity Monitor for idle CPU, memory, energy.
- Budgets enforced manually each milestone.

---

## 9. Distribution and operations

- Apple Developer Program ($99/year), Developer ID Application, hardened runtime, no App Sandbox (Accessibility).
- Notarize with `notarytool`, staple, `spctl --assess`.
- Tauri bundler: `.app`, `.dmg`; universal binary (arm64 + x86_64); macOS 14+.
- Tauri updater: minisign keys, HTTPS endpoint, stable/beta; manual check + automatic.
- Homebrew Cask, GitHub Releases, checksums.
- No Mac App Store.
- Crash reporting opt-in only; no analytics.
- Support: GitHub Discussions/Issues, diagnostics export, app matrix, `SECURITY.md`.

### Development signing

- Create a stable self-signed "Elocutio Dev" certificate and fixed bundle ID so TCC permissions survive rebuilds.
- Document the exact workflow; do not rely on ad-hoc signing.

---

## 10. Cost model

### Development

| Item | Cost |
| --- | --- |
| Apple Developer Program | $99/year |
| CI (GitHub Actions macOS) | $0–50/month |
| Domain | ~$15/year |
| Design assets | $0–500 one-time |
| **Total to launch** | **~$150–700** |

### Runtime (user-paid)

Using the research assumptions (email: 600 input / 450 output tokens; document: 3,600 input / 1,400 output across four chunks):

| Model | Email /1k reviews | Doc /1k reviews |
| --- | ---: | ---: |
| DeepSeek v4 Flash | $0.14 | $0.58 |
| **Claude Haiku 5.5 (default)** | **$0.28** | **$1.06** |
| Gemini 3.1 Flash Lite | $0.82 | $3.00 |
| GPT-5-mini | $1.05 | $3.70 |
| Claude Sonnet 5 | $5.70 | $21.20 |
| GPT-5.4 | $8.25 | $30.00 |
| GPT-5.6-sol / Opus 5.5 | $11.40 | $42.40 |
| Blended 85% Haiku + 15% Sonnet | $1.14 | $4.24 |

Show the estimate before long analyses; cap monthly spend; default to Haiku with `reasoning.effort: none`.

**Voice overhead:** the style card + exemplars add ~300–500 prompt tokens per voice-aware rewrite. At Haiku 5.5 prices that is under $0.0001 per rewrite. Sample ingestion and feature extraction are local and free; optional LLM sample analysis is opt-in with a preview and cost estimate.

### Sustainability (post-1.0)

- Donations/GitHub Sponsors.
- Optional hosted team style-guide sync (open protocol).
- Support contracts.
- Never paywall core writing features.

---

## 11. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Fork inherits beta bugs | Pin a tag, audit, add tests, keep upstream sync, be ready to patch |
| Tauri UI feels non-native | Custom design system, native overlay windows, small webviews only for settings/panels |
| Highlighter/LLM IPC complexity | Define a versioned JSON protocol early; fake-provider and integration tests |
| Overlay misalignment | Reuse Harper geometry; golden screenshots; widget-first fallback |
| Text injection corruption | Re-read immediately before injecting; pasteboard+⌘V primary, AX fast path only in settable AppKit fields; read-back verify; abort on mismatch; undo; never secure fields |
| Latency/cost | Local rules instant; LLM debounced/on demand; cache; cheap default; caps |
| Bad model spans | Quote-based schema, local resolution, ambiguity rejection, repair loop |
| Prompt injection via document text | Text-as-data delimiters; no tools in analysis; structured output only |
| Key leakage | Main-process Keychain, no logs, redaction, no keys in highlighter |
| Local bridge exposure | 127.0.0.1, pairing token, Origin allowlist, close on quit |
| TCC resets in dev | Stable dev cert + fixed bundle ID |
| Notarization/updater surprises | Set up in M0, automate release |
| Harper trademark/attribution | Rename everything, retain Apache-2.0/NOTICE, clear "built on Harper" |
| Upstream divergence | Small, well-documented patches; contribute generic fixes upstream |
| Scope creep | Phase gates, non-goals, "later" list |
| Maintainer bandwidth | Modular crates, contributor docs, focused MVP |
| Voice profile feels creepy or overfit | Off by default; transparent style card; strength slider; freeze/reset; cap single-sample influence |
| Voice copies sample content | 8-gram overlap guard; reject/regenerate; never inject full samples |
| Voice feedback loop converges on AI text | Learn from user-authored text/edits; down-weight unedited AI acceptances |
| Voice samples leak | Local encryption; excerpt-only prompts; style-card-only mode; preview; delete cascades |
| Voice parsing produces garbage | Quality gate; extracted-text preview; reject low-quality files |
| Voice profile drifts after a job change | Drift detection; "update your voice?" prompt; versioned profiles |
| Impersonation via uploaded samples | User attestation; brand-profile labeling; acceptable-use policy |

---

## 12. Open decisions

| # | Decision | Options | Recommendation |
| --- | --- | --- | --- |
| 1 | Build strategy | Fork Harper / native Swift + harper-core / clean-room | **Decided: Fork Harper** (Option A, approved 2026-10-19) |
| 2 | Project name | Elocutio / Inkwell / Quill / other | **Elocutio** (working); clear trademark |
| 3 | License | Apache-2.0 / AGPL-3.0 | **Apache-2.0** (required for Harper compatibility) |
| 4 | Default provider | OpenRouter / OpenAI / local | **OpenRouter** |
| 5 | Default model | Haiku 5.5 / DeepSeek / Gemini Lite | **Claude Haiku 5.5**, budget option DeepSeek |
| 6 | Minimum macOS | 14 / 15 / 26 | **14**, Apple FM gated to 26 |
| 7 | Browser extension | Chrome/Edge first / Safari too | **Chrome/Edge first** |
| 8 | Telemetry | None / opt-in crash reports | **None by default** |
| 9 | Docs editor | Build / defer | **Defer** |
| 10 | Plagiarism/AI detection | Omit / substitute | **Omit scores**; web similarity + citations later |
| 11 | Voice learning approach | Fine-tune / style card + retrieval | **Style card + feedback + exemplars**; local LoRA later |
| 12 | Raw voice samples | Encrypted raw / features only | **Encrypted raw by default**, "features only" option |
| 13 | Voice sample formats | txt+md+docx / +pdf / +email | **txt+md+docx+paste in v1**, PDF/email later |
| 14 | Platform order | macOS → browser → Windows → mobile | **macOS v1, Chrome/Edge in M3, Windows next, iOS/Android after** |
| 15 | Cross-device profiles | Export/import / encrypted sync | **Export/import in v1**, E2E sync later |

---

## 13. Immediate next steps (after approval)

1. Confirm name and license; Option A (Harper fork) is approved.
2. Fork Harper v2.13.0; rebrand; build and run on this Mac.
3. Run the M0 spike: overlay in 5 apps, OpenRouter key validation, rewrite selection.
4. Write the IPC protocol and the suggestion contract.
5. Implement the offset resolver + golden set **before** polishing UI.
6. Define the voice profile schema and feature extractor alongside the resolver; voice is a core differentiator, not a bolt-on.
7. Ship an internal M1 to 5 users in Mail, Notes, Safari, Chrome, Word.
8. Iterate on false positives/latency; then build the widget, voice v0, and extension.

---

## 14. Definition of done for v1.0

**macOS v1:**

- A new user can install from a notarized DMG, choose local-only or OpenRouter, grant Accessibility, and get a correct suggestion in under 3 minutes.
- 12 Tier A apps pass the manual matrix on current and previous macOS.
- Local rules respond in <50 ms; LLM suggestions are explainable and minimally invasive; accept always edits the intended span.
- No telemetry or hidden network calls; the network log proves it.
- Keys are in the Keychain, never logged, removable; the highlighter has no key access.
- Chrome/Edge extension works on Gmail, Notion, Slack web, Linear, and a plain textarea; Google Docs is explicitly unsupported with a clear fallback.
- Voice: a user can add 3 samples or 5 edits, see a clear style card, preview a rewrite, and rate it "sounds like me" ≥4/5; no sample content is copied; samples are encrypted and deletable.
- Signed updates work from a clean install.
- Apache-2.0 with Harper attribution, contributing guide, code of conduct, security policy, support matrix, and public roadmap.

**Platform expansion:** each platform has its own definition of done, tested app matrix, and distribution path. Windows must reuse the shared core; iOS must not claim cross-app access; Android must pass Play policy review for any AccessibilityService use. The platform matrix lives in [`apps/README.md`](../apps/README.md).
