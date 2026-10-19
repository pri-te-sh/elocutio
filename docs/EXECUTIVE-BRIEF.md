# Executive Brief — Elocutio

**A private, open-source writing companion for macOS — with Windows, browser, iOS, and Android on the roadmap — powered by the model you choose.**

Prepared 19 October 2026 · Product: Elocutio · License: Apache-2.0

---

## The opportunity

Grammarly is now part of **Superhuman**, an agent platform. Its writing assistant is excellent, but it is closed, subscription-based, and cloud-only: there is no documented offline mode, and enterprise "BYOK" means bring-your-own-encryption-key — never bring-your-own-model. You cannot choose your model, keep your text on your Mac, or avoid the $12–30/user/month fee.

At the same time, three things have changed:

1. Cheap models now produce excellent proofreading, clarity, tone, and rewrite suggestions via API.
2. Local models (Harper, Apple Foundation Models, Ollama) are good enough for instant correctness.
3. **Harper Desktop already solved the hardest macOS problem** — a system-wide Accessibility overlay with sub-10 ms local checks — and released it under Apache-2.0.

That creates an unoccupied position: **Harper's local-first macOS ergonomics + Grammarly's paid-tier judgment + your own API key.**

---

## What we learned

| Finding | Implication |
| --- | --- |
| The competitor is Superhuman, not 2023 Grammarly | Position on privacy, model choice, and rewriting quality — not "better underlines" |
| Grammarly's paywall is at **judgment**, not correctness | Local correctness is table stakes; win on clarity, tone, rewrites, compose, translate, humanize |
| Harper Desktop occupies "private, free, system-wide grammar" | Do not rebuild it. Fork it (Apache-2.0) and add the LLM layer it explicitly refuses |
| The macOS mechanism is an Accessibility overlay, not an IME | Budget for per-app breakage, canvas editors, secure fields, OS updates |
| AX writes are ignored by Chromium | **Pasteboard + ⌘V is the primary write path**; AX is an AppKit fast path |
| Google Docs is canvas-rendered | Explicitly unsupported in v1; Gmail/Notion/Slack/Linear via extension |
| Plagiarism and AI detection are data businesses | Do not fake them; offer local corpus overlap, web similarity, AI-phrase linting, and provenance |
| Grammarly's personalization is cloud-side | A local, explainable voice profile is a second differentiator |

---

## What we're building (v1)

**A shared Rust core with thin platform shells, starting with a macOS menu-bar app + Chrome/Edge extension that:**

- proofreads the focused field using Harper locally (<10 ms) and your model for judgment;
- shows grammar, clarity, conciseness, tone, and rewrite suggestions in place;
- detects tone and rewrites to a target tone, with a diff you approve;
- composes, replies, summarizes, translates, and humanizes on demand;
- **learns your voice** from uploaded samples (`.txt`/`.md`/`.docx`/paste in v1) and from the edits you accept, applies it to rewrites and drafts, and shows you exactly what it learned;
- supports OpenRouter by default, plus OpenAI-compatible, Anthropic, Gemini, and Ollama;
- keeps keys in the macOS Keychain, never logs text, and shows a network/cost log;
- works offline in local-only mode;
- lets you bring a style guide, brand voice, glossary, snippets, and dictionary;
- keeps voice samples encrypted locally and off by default.

**Not in v1:** Google Docs, plagiarism/AI-detector scores, enterprise SSO/SCIM/DLP, Safari extension. Windows, iOS, and Android follow the macOS release; the core is designed for them from day one.

## Platforms

| Platform | Timing | Approach |
| --- | --- | --- |
| macOS | v1 | Tauri + Harper Accessibility overlay |
| Chrome/Edge | M3 | MV3 extension + local bridge |
| Windows | After v1 | Tauri + UI Automation |
| iOS | Planned | SwiftUI + keyboard/share extensions |
| Android | Planned | Compose + IME/AccessibilityService |
| Safari/Firefox | Later | WebExtension adaptation |

The shared Rust crates carry the analysis engine, providers, voice profiles, and storage. Each platform adds only capture, overlay, injection, permissions, and distribution.

---

## Architecture in one picture

```text
┌──────────────── Tauri main process (Rust) ────────────────┐
│ tray · updater · config · SQLite · Keychain               │
│ providers (OpenRouter / OpenAI / Anthropic / Gemini /     │
│            Ollama)                                        │
│ analysis: chunk → prompt → validate → resolve → dedupe    │
│ voice: samples → style card → exemplars → prompt          │
│ bridge: 127.0.0.1 WebSocket + pairing token               │
└──────────────────────────┬────────────────────────────────┘
                           │ IPC (JSON lines)
┌──────────────────────────▼────────────────────────────────┐
│ Highlighter subprocess (Rust + native macOS)              │
│ Accessibility read · Harper local rules · overlays        │
│ non-activating card/widget · paste/AX injection           │
└───────────────────────────────────────────────────────────┘
```

**Key design choices**

- **Fork Harper** for the shell, AX broker, highlighter, and local rules; build the LLM/BYOK layer ourselves.
- **Two processes:** main owns secrets/network; highlighter owns text/injection.
- **Quotes, not offsets:** the model returns verbatim quotes; the client resolves spans exactly/normalized/fuzzy and drops anything ambiguous.
- **Never steal focus:** non-activating panels; click-only cards.
- **Pasteboard primary, AX fast path:** works in Chromium/Electron and AppKit.
- **Local + LLM merge:** instant correctness, debounced judgment.

---

## Timeline and effort

| Phase | Outcome | Days |
| --- | --- | --- |
| M0 | Fork, rebrand, 5-app spike, OpenRouter validation, rewrite a selection | 5–7 |
| M1 | LLM MVP: suggestions, card, accept/dismiss, tone, onboarding, Keychain, usage | 10–14 |
| M2 | Inline UX + voice v0: widget, underlines, rewrites, cache, blocklist; profile from feedback | 13–18 |
| M3 | Chrome/Edge extension, assistant, compose/reply/summarize, multi-provider; voice uploads | 19–25 |
| M4 | Style guide, brand voice, glossary, snippets, provenance, history, translation; per-context voices | 14–20 |
| M5 | Notarized DMG, signed updates, Homebrew, docs, security review | 8–12 |
| **Total** | **v1.0** | **69–96 (~14–19 weeks)** |

MVP in ~4–6 weeks. Public beta in ~10–13 weeks. Voice learning adds ~11–14 engineering days across M2–M4. One experienced Rust/TypeScript engineer, full-time; add 30–50% part-time.

---

## Cost

**Development:** ~$150–700 to launch (Apple Developer Program $99/year, domain, CI, optional design).

**Runtime (user-paid, at October 2026 prices):**

| Model | Per 1,000 email reviews | Per 1,000 1,500-word docs |
| --- | ---: | ---: |
| DeepSeek v4 Flash | $0.14 | $0.58 |
| **Claude Haiku 5.5 (default)** | **$0.28** | **$1.06** |
| Gemini 3.1 Flash Lite | $0.82 | $3.00 |
| Claude Sonnet 5 | $5.70 | $21.20 |

No subscription, no markup. Local mode is free.

---

## Biggest risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Fork inherits beta bugs | Pin v2.13.0, audit, test, keep upstream sync, be ready to patch |
| Focus stealing breaks paste | Non-activating panels, click-only cards, audit every window call |
| Text corruption on accept | Re-read before inject, paste primary, read-back verify, abort on mismatch |
| Overlay misalignment | Reuse Harper geometry, golden screenshots, widget-first fallback |
| Latency/cost | Local rules instant, LLM debounced/on demand, cache, cheap default, caps |
| Bad model spans | Quote-based schema, local resolution, ambiguity rejection, repair loop |
| Harper trademark/attribution | Rename everything, retain Apache-2.0/NOTICE, clear "built on Harper" |
| Google Docs requests | Explicit unsupported message + copy-into-editor path |
| Voice profile feels creepy/overfit | Off by default, transparent style card, strength slider, freeze/reset, cap single-sample influence |
| Voice copies sample text | 8-gram overlap guard, excerpt-only prompts, reject/regenerate |
| Voice samples leak | Encrypted at rest, Keychain-wrapped key, style-card-only mode, preview, delete cascades |

---

## Decisions needed before build

1. **Build strategy:** fork Harper (recommended) vs native Swift + harper-core.
2. **Name:** Elocutio (working) vs another name; trademark clearance before launch.
3. **License:** Apache-2.0 (recommended) vs AGPL-3.0.
4. **MVP emphasis:** selection-first + local rules (recommended) vs live-first.
5. **Default model:** Claude Haiku 5.5 (recommended) vs DeepSeek budget default.
6. **Voice sample storage:** encrypted raw text (recommended) vs features-only.

---

## Next steps

1. Approve the strategy and scope.
2. Fork Harper v2.13.0, rebrand, and run the M0 spike on this Mac.
3. Write the IPC protocol and suggestion contract.
4. Build the offset resolver and golden set before polishing UI.
5. Build the voice profile schema and feature extractor alongside the resolver; define the copy guard and feedback weights.
6. Ship an internal M1 to 5 users in Mail, Notes, Safari, Chrome, Word.

---

**Read more:** [Brand](BRAND.md) · [Product audit](AUDIT.md) · [Full plan](PLAN.md) · [Architecture](ARCHITECTURE.md) · [Feature matrix](FEATURE-MATRIX.md) · [Decisions](DECISIONS.md) · [Voice learning](VOICE-LEARNING.md) · [Prompts](PROMPTS.md)
