# Architecture Decision Records

Short, dated records of the choices that shape the build. Format: context → decision → consequences → alternatives.

---

## ADR-001 — Fork Harper Desktop instead of rebuilding the macOS shell

**Date:** 2026-10-19
**Status:** **Accepted** (2026-10-19; Harper fork path approved)

**Context.** The hardest and least differentiated part of a Grammarly-like macOS app is the Accessibility plumbing: reading the focused field, drawing underlines over foreign windows, tracking window movement, handling per-app enablement, and replacing text safely. Harper Desktop is Apache-2.0, ships this exact stack, and has a fast local English grammar engine. The differentiated work is the LLM/BYOK judgment layer, the UX, and the extension.

**Decision.** Fork `Automattic/harper` at v2.13.0, rebrand the desktop app, keep `harper-core`, and build the provider/analysis/UX layer as our original contribution. Keep an `upstream` remote and document the sync policy.

**Consequences.**

- Faster to a working overlay (weeks, not months).
- We inherit beta bugs, Rust + TypeScript + ObjC complexity, and a large monorepo.
- We must remove all Harper trademarks and retain Apache-2.0 notices.
- We must audit inherited network/IPC/updater code.
- If the M0 spike fails, switch to ADR-001B (native Swift + harper-core).

**Alternatives.** Native Swift app + harper-core via FFI (more original, 4–8 weeks slower); clean-room native Swift + own rules (slowest, weakest local grammar).

---

## ADR-001B — Fallback: native Swift app + harper-core FFI

**Date:** 2026-10-19
**Status:** Fallback

**Context.** If the Tauri/highlighter shell cannot deliver non-activating, correctly aligned UI in the Tier A apps, the fork becomes a liability.

**Decision.** Fall back to a native SwiftUI/AppKit app that links `harper-core` as a Rust static library (UniFFI or C FFI), reusing Harper Desktop's AX code as a reference. Keep the same provider/analysis contracts.

**Consequences.** Better native UX/performance; 6–10 more weeks; no Windows path; Rust FFI build complexity.

---

## ADR-002 — Apache-2.0 license

**Date:** 2026-10-19
**Status:** **Accepted** (2026-10-19; repository licensed Apache-2.0)

**Context.** The project is derived from Apache-2.0 Harper. We want adoption, contributions, and commercial compatibility.

**Decision.** License our code under Apache-2.0; docs under CC BY 4.0. Retain Harper's `LICENSE` and add a `NOTICE` with attribution and our modifications. No copyleft.

**Consequences.** Compatible with the fork; permissive for integrations; a competitor could host a closed service, which we accept because the moat is trust/execution, not code exclusivity.

**Alternatives.** AGPL-3.0 (protects hosted forks, but is incompatible with a permissive upstream fork in the direction we need and can deter contributors); GPL-3.0 (same).

---

## ADR-003 — Tauri/Rust shell, native overlay, Svelte webviews

**Date:** 2026-10-19
**Status:** Proposed

**Context.** We inherit Harper's Tauri 2 + Rust + Svelte shell. The overlay must be native (non-activating panels, click-through underlines); settings/assistant panels can be webviews.

**Decision.** Keep Rust/Tauri for lifecycle, tray, updater, IPC, and the highlighter; render underlines/widget/cards with the inherited Rust/egui or native AppKit panels; render settings/onboarding/assistant with Svelte webviews.

**Consequences.** Fast iteration on settings/assistant; native behavior where it matters; three languages to maintain. The assistant panel may take focus (it is a deliberate interaction), but the card may not.

**Alternatives.** All-native SwiftUI (slower, but cleaner); all-web Electron (worse focus/AX integration, higher memory).

---

## ADR-004 — OpenRouter default; Claude Haiku 5.5 as default model

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Users must bring a key. OpenRouter gives one key access to hundreds of models and supports structured outputs, routing, and cost reporting. The default model must be cheap, fast, structured-output-capable, and able to disable reasoning.

**Decision.** Default provider: OpenRouter (`https://openrouter.ai/api/v1`). Default model: `anthropic/claude-haiku-5.5` ($0.10/$0.50 per 1M, 1M context, structured outputs, `reasoning.effort: "none"`). Escalation: `claude-sonnet-5`, then `gpt-5.6-sol` / `claude-opus-5.5`. Budget option: `deepseek/deepseek-v4.1-flash`. Always set `provider.require_parameters: true` for structured output.

**Consequences.** ~$0.28 per 1,000 email reviews on the default. Users can switch; the app never silently upgrades models. OpenRouter-specific fields are isolated in the adapter.

**Alternatives.** OpenAI direct (fewer models, no unified billing); local-only default (privacy but weaker quality); Gemini Lite (cheaper but reasoning mandatory, more latency).

---

## ADR-005 — Hybrid local + LLM engine

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Cloud round-trips cannot keep up with typing, and sending every keystroke is costly and privacy-hostile. Grammarly's own latency comes from datacenter models plus a mature incremental engine. Harper provides a <10 ms local engine.

**Decision.** Run Harper locally on every keystroke for correctness. Run the LLM only on an explicit shortcut, after a 700 ms pause in opted-in live mode, or for explicit rewrite/compose actions. Merge and dedupe the two suggestion sets; local wins for deterministic correctness, LLM wins for judgment.

**Consequences.** Instant feedback, lower cost, offline correctness. Two suggestion sources to merge and explain. Local rules are English-only.

**Alternatives.** LLM-only (latency/cost/privacy); local-only (no judgment, the Harper position we are differentiating from).

---

## ADR-006 — The model returns quotes, not offsets

**Date:** 2026-10-19
**Status:** Proposed

**Context.** LLMs cannot count UTF-16 or grapheme offsets reliably, especially with emoji, combining marks, CRLF, and RTL. A wrong offset corrupts the user's text.

**Decision.** The model returns `paragraph_id`, `original` (verbatim quote), `replacement`, `category`, `severity`, and `explanation`. The client resolves offsets: exact → normalized → fuzzy (≥0.85) → drop. Store both UTF-16 and grapheme offsets. No self-reported confidence.

**Consequences.** Robust edits; occasional dropped suggestions; the resolver becomes a core tested component. Slightly larger prompts (quotes) but safer.

**Alternatives.** Model-provided offsets (fragile); full rewritten text + diff (over-edits, loses formatting); character-index prompts (model miscounts).

---

## ADR-007 — Pasteboard + ⌘V is the primary write path; AX is a fast path

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Chromium's AX implementation ignores `AXValue`/`AXSelectedText` writes (confirmed in Chromium source). `kAXSelectedTextAttribute` is not writable. `CGEventKeyboardSetUnicodeString` may be ignored. AX whole-field writes destroy formatting/undo.

**Decision.** Re-read the field immediately before injecting. Use the AX fast path only in settable, non-Chromium, short AppKit fields. Otherwise pasteboard + ⌘V with transient/concealed tags, restore-by-item, and read-back verification. Abort on mismatch. Never inject into secure fields.

**Consequences.** Works in Chrome/Electron/Slack/Notion/VS Code; clipboard managers may race (mitigated by tags/changeCount); paste is one undo group; ~60 ms latency.

**Alternatives.** AX-only (fails in browsers); unicode key events (unreliable, breaks IME); IME (too invasive, not v1).

---

## ADR-008 — Non-activating, click-only suggestion cards

**Date:** 2026-10-19
**Status:** Proposed

**Context.** If the card becomes key or activates the app, the caret leaves the field and the subsequent ⌘V lands nowhere. This is the single biggest product-killer in this category.

**Decision.** All suggestion surfaces are non-activating panels with `canBecomeKey = false`, `canBecomeMain = false`, `.nonactivatingPanel`, and collection behavior `[.canJoinAllSpaces, .fullScreenAuxiliary, .transient, .ignoresCycle]`. Cards are click-only; no text input. The assistant panel may become key, but restores focus afterward.

**Consequences.** Reliable paste; no in-card custom prompt field (use the assistant panel); every inherited window call must be audited.

**Alternatives.** Activating popovers (breaks paste); global key capture while the card is visible (needs Input Monitoring, risky).

---

## ADR-009 — Google Docs is unsupported in v1

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Google Docs has used canvas rendering since 2021. Neither Accessibility nor content scripts reliably read or decorate the text. Grammarly supports Docs through a browser extension, but that is a substantial special integration.

**Decision.** Declare Google Docs unsupported in v1. Detect it and show a clear message with a "copy into Elocutio's editor" path. Revisit with a Docs Add-on or API integration post-1.0.

**Consequences.** A visible gap for a common app; honest messaging; avoids a time sink. Gmail, Notion, Slack, and Linear remain supported via the extension.

**Alternatives.** Canvas OCR (Screen Recording, poor accuracy); Docs Add-on (separate platform, Google review); unofficial DOM hacks (fragile).

---

## ADR-010 — No plagiarism or AI-detection verdicts

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Plagiarism requires a web-scale licensed corpus; AI detection requires proprietary labeled data and continual retraining, and is contested. Grammarly acquired GPTZero to shore up its version. Shipping a confident local score would be misleading and reputationally risky.

**Decision.** Do not ship a plagiarism checker or an AI-detector score. Offer: (a) local MinHash overlap against user-supplied corpora, (b) optional web-similarity via the user's search API, clearly labeled, (c) AI-phrase linting as style feedback, (d) a local provenance log. Never market detector evasion.

**Consequences.** We lose a checkbox vs competitors; we gain trust and avoid a data business. Some users will be disappointed; explain why.

**Alternatives.** Integrate a third-party detector via BYOK (optional later); train a small classifier (stale, unethical at scale).

---

## ADR-011 — Localhost WebSocket bridge first; native messaging later

**Date:** 2026-10-19
**Status:** Proposed

**Context.** The browser extension needs to talk to the app. Native messaging is browser-enforced and has no network surface, but requires per-browser manifests and a host process that relays to the app. A local WebSocket is one code path for all browsers and easy to debug.

**Decision.** Ship a loopback WebSocket (`127.0.0.1:43110`) with a 6-digit pairing flow, a 256-bit bearer token, and an Origin allowlist. Add native messaging post-1.0 for robustness and enterprise security preferences. Keep the protocol identical so the transport can change.

**Consequences.** Simple pairing and debugging; we must implement auth/origin checks correctly; a local port is open while the app runs. Close it on quit/disable.

**Alternatives.** Native messaging only (more setup, better security); no bridge (no web editor support).

---

## ADR-012 — No telemetry; local network log instead

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Privacy is the wedge. Users cannot verify "we don't send your text" if the app phones home. At the same time, debugging provider failures needs diagnostics.

**Decision.** No analytics, no crash reporting by default. Provide an in-app request log (host, model, tokens, cost, status; no text) and a diagnostics export the user can inspect before sharing. Opt-in crash reports at launch, scrubbed.

**Consequences.** Less product telemetry; slower feedback loops; stronger trust story. Support relies on user-provided diagnostics.

**Alternatives.** Opt-out telemetry (violates the positioning); Sentry/crash SDKs (third-party data path).

---

## ADR-013 — API keys in the Data Protection Keychain

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Keys must survive restarts, never sync to iCloud, and never appear in logs. The legacy login keychain can sync; the Data Protection keychain has stronger, device-only semantics.

**Decision.** Store keys with `kSecClassGenericPassword`, `kSecUseDataProtectionKeychain: true`, `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`, `kSecAttrSynchronizable: false`, optional `.userPresence` access control. Keys live only in the main process; the highlighter never receives them.

**Consequences.** Slightly more complex Security framework code; keys are device-bound; users must re-enter on a new Mac.

**Alternatives.** `UserDefaults` (insecure); environment variables (leak in process listings); iCloud Keychain (syncs secrets).

---

## ADR-014 — Natural-language preferences over dozens of toggles

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Grammarly is moving from fixed suggestion categories to prompt-steerable agent preferences ("tell it which underlines matter"). A 40-toggle settings page is both expensive to build and worse UX.

**Decision.** Ship a small set of category toggles plus a natural-language preferences field ("only grammar and clarity; use American English; never suggest em dashes"). Parse preferences into a structured config + prompt preamble; show the parsed result so the user can correct it.

**Consequences.** Cheaper than 40 toggles; more flexible; requires a parser/LLM call and a preview UI. Some users still want explicit toggles, so keep the top categories.

**Alternatives.** Full toggle matrix (maintenance burden); no preferences (generic suggestions).

---

## ADR-015 — Provenance instead of AI detection

**Date:** 2026-10-19
**Status:** Proposed

**Context.** "Did AI write this?" is unreliable and harmful when wrong. "How was this written?" is locally observable and useful, especially in education.

**Decision.** Offer an opt-in local provenance log: typed, pasted (content hash, not content), AI-inserted, AI-replaced, accepted, dismissed. Export a signed report. Do not infer authorship or accuse.

**Consequences.** A genuinely differentiated, honest feature; limited appeal outside education/editing; privacy-sensitive (off by default).

**Alternatives.** AI detector score (rejected); no provenance (missed differentiator).

---

## ADR-016 — The browser extension is part of v1, but Docs is not

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Knowledge workers write in the browser. AX is reliable for reading but fragile for writing in Chromium; the extension is the reliable path. Google Docs is the exception because of canvas rendering.

**Decision.** Ship a Chrome/Edge MV3 extension in v1 for Gmail, Notion, Slack web, Linear, and generic textareas/contenteditable. Do not ship Google Docs support. Add Safari post-1.0 inside the same notarized app.

**Consequences.** Covers the majority of web writing; leaves a visible Docs gap; one codebase with a polyfill.

**Alternatives.** Native-only (worse browser UX); Docs-first extension (high effort, uncertain reliability).

---

## ADR-017 — Voice learning via a local style card + retrieval, not fine-tuning

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Users want Elocutio to learn their tone and nuances over time and to learn from uploaded writing. The intuitive implementation — fine-tune a model on the user's corpus — is a poor fit for a BYOK app: most providers do not expose per-user fine-tuning, the user may switch models at any time, training is slow and costly, and uploading a corpus conflicts with the local-first premise.

**Decision.** Build a local **Voice Profile** from three sources: uploaded samples, accepted/dismissed/edited suggestions, and explicit feedback. Store a structured style card (rhythm, diction, structure, punctuation, tone, person) plus 2–4 representative exemplars. Inject the card and exemplars into rewrite/compose/reply prompts; instruct the model to match voice without copying content. Never fine-tune a cloud model on user writing in v1. Evaluate local LoRA for Ollama/MLX as a post-1.0 power-user feature.

**Consequences.**

- Provider-agnostic: the profile travels across models.
- Private: only the card and selected excerpts leave the Mac; raw samples stay local.
- Explainable and editable: every trait has confidence and evidence.
- Adds ~11–14 engineering days across M2–M4.
- Not true model adaptation; complex nuances may be approximated rather than learned.
- Requires copy guards, feedback-loop protection, and drift detection.

**Alternatives.** Cloud fine-tuning (rejected: privacy, cost, provider lock-in); local LoRA only (later; limited to local models); prompt-only preferences (weaker; no learning over time); no voice feature (missed differentiator).

---

## ADR-018 — Encrypted raw voice samples by default, with a features-only mode

**Date:** 2026-10-19
**Status:** Proposed

**Context.** Uploaded samples are among the most sensitive data the app holds. Storing only derived features is safer but prevents reprocessing when the extractor improves; storing raw text enables better profiles but requires protection.

**Decision.** Store raw samples locally, encrypted at rest with AES-256-GCM; the data key is wrapped by the macOS Keychain. Provide a "features only" mode that stores no raw text. Exemplars are user-reviewable excerpts. Deleting a sample removes its exemplars and its contribution to the profile; deleting a profile removes everything.

**Consequences.** Better profiles and reprocessing; key management and migration complexity; backups may retain deleted data outside the app's control (documented). Users who choose features-only cannot reprocess without re-uploading.

**Alternatives.** Features-only default (more private, less capable); plaintext storage (unacceptable); SQLCipher for the whole DB (heavier; complicates migrations).

---

## ADR-019 — Voice learning is opt-in, and AI-generated text is down-weighted

**Date:** 2026-10-19
**Status:** Proposed

**Context.** A voice profile that learns from everything the app sees is creepy, and one that learns from accepted AI rewrites converges on the model's voice rather than the user's.

**Decision.** Voice learning is off by default. When enabled, learn primarily from user-authored text and user edits; weight unedited accepted AI text at 0.1; never learn from secure fields, excluded apps, or deleted samples. Show every inferred preference with an undo. Provide freeze and reset.

**Consequences.** Slower profile growth; more trustworthy behavior; avoids a feedback loop; requires clear onboarding and controls.

**Alternatives.** Learn from all text (fast but creepy and self-reinforcing); learn only from uploads (limited); no learning (missed differentiator).

---

## ADR-020 — Product name: Elocutio

**Date:** 2026-10-19
**Status:** **Accepted**

**Context.** The project needed a distinctive name that fits a private, model-agnostic writing assistant with voice learning; it must be usable as a macOS bundle ID, GitHub organization, npm package, and domain; and it should not collide with existing writing products.

**Decision.** Name the product **Elocutio** (pronounced *el-oh-KOO-tee-oh*), the classical rhetorical canon of style and expression. Planned primary domain `elocutio.app` (available at decision time; registration pending), docs at `elocutio.dev`, npm `elocutio`, GitHub organization `elocutio-app`, bundle ID `app.elocutio.desktop`.

**Evidence at decision time (19 Oct 2026):**

- 0 US trademark results for the exact word in Trademarkia's public search.
- No significant existing product in the writing space; GitHub has a few zero-star repos, no competing app.
- `elocutio.app`, `elocutio.dev`, and `elocutio.io` were available; `elocutio.com` is registered (blank site).
- npm `elocutio` was available; the GitHub username `elocutio` exists but has no product repository.

**Consequences.**

- Distinctive, editorial, and semantically precise: Elocutio is the art of choosing words and shaping tone — exactly the product's job.
- Four syllables and an uncommon Latin root create a small pronunciation/spelling barrier; mitigate with a pronunciation guide and the "elo-" root.
- `.com` is unavailable; `.app` is appropriate for a macOS app.
- The GitHub username is taken; use an organization variant.
- Public trademark search is a signal, not clearance; complete US/EU/UK searches in classes 9 and 42 and file an intent-to-use application before launch.

**Alternatives considered.** **Stet** (proofreader's mark "let it stand"; excellent meaning, but prime domains taken, `stet/stet` on GitHub, and a Google security tool named Stet); **Pilcrow** (paragraph mark; existing open-source Markdown editor and iOS document editor use the name, domains taken); **Locution** (existing macOS dictation app, npm/GitHub taken); **Faircopy** (existing word processor for humanities scholars); **Timbre** (44 US trademark records, crowded audio use); **Tacet** (live Class 009 software trademark filed Oct 2026).

---

## ADR-021 — Shared Rust core with thin platform shells; macOS first

**Date:** 2026-10-19
**Status:** **Accepted**

**Context.** The product starts on macOS but will ship a browser extension, a Windows app, and iOS/Android apps. Rebuilding analysis, providers, voice, and storage per platform would fragment quality, double the prompt/validation work, and make voice profiles non-portable. At the same time, text capture, overlays, replacement, permissions, and distribution are unavoidably platform-specific.

**Decision.** Put all product logic in platform-agnostic Rust crates (`elocutio-core`, `elocutio-analysis`, `elocutio-llm`, `elocutio-voice`, `elocutio-store`, `elocutio-bridge`, `elocutio-platform` traits). Give each platform a thin shell for capture, overlay, injection, permissions, and distribution. Ship macOS v1 first, then Chrome/Edge in M3, Windows next, and iOS/Android after the desktop apps. Voice profiles are portable JSON with export/import; cloud sync is not v1.

**Consequences.**

- One analysis engine and one prompt/validation contract across platforms.
- Platform work is bounded and testable against shared fixtures.
- Rust FFI (UniFFI/JNI) and per-platform packaging are new costs on mobile.
- iOS cannot read other apps' text; the keyboard/share extension is the product.
- Android AccessibilityService is policy-sensitive; the keyboard is the reliable fallback.
- No platform is announced as supported until it has a tested app matrix.

**Alternatives.** Duplicate the engine per platform (quality drift, maintenance cost); a cloud backend for all platforms (violates the local-first premise); web-only (loses system-wide access).

---

## ADR-022 — Brand system v1: Ink Indigo, Inter, twin-quote mark

**Date:** 2026-10-19
**Status:** **Accepted**

**Context.** The product needed a distinctive identity that reads as an editor, not a grammar checker, and works across macOS, Windows, browser, iOS, and Android.

**Decision.** Adopt the twin-quote mark with the raised eyebrow, the tagline **"Your voice. Clearly expressed."**, the palette **Ink Indigo `#191F70`, Near Black `#171717`, Warm Paper `#F8F7F3`, White `#FFFFFF`**, and **Inter** as the primary typeface. Keep the mark flat and preserve the eyebrow and asymmetry. Generate platform icons from source artwork with `scripts/build-assets.py`. Full rules: [`BRAND.md`](BRAND.md).

**Consequences.**

- Consistent identity across platforms and store listings.
- The menu-bar icon needs a simplified monochrome template at 16–18 px.
- App icons are generated, not hand-edited, so they stay consistent.
- The design tokens are a single source of truth for web and native UI.
- The name and mark still require formal trademark clearance.

**Alternatives.** A wordmark-only identity (weaker at small sizes); a generic quote mark (loses the eyebrow and the editorial wink); system fonts only (less distinctive across platforms).
