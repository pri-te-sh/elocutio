# Feature Matrix — Grammarly parity plan

**Legend**

- **Target:** `Full` = match the core behavior; `Good` = useful equivalent, not identical; `Substitute` = different mechanism, clearly labeled; `Omit` = not building; `N/A` = not applicable to our product.
- **Phase:** M1 = LLM MVP; M2 = inline UX; M3 = extension/assistant; M4 = personal/trust; M5 = launch; `Later` = post-1.0; `Never` = explicitly not building.
- **Effort:** S ≤2 days; M 3–5 days; L 6–10 days; XL >10 days (one experienced engineer).
- Tier facts are from Grammarly's public plans page, October 2026; enterprise features are sales-gated.

---

## Summary

| Category | Features | Full/Good | Substitute | Omit/Never |
| --- | ---: | ---: | ---: | ---: |
| Correctness | 10 | 10 | 0 | 0 |
| Clarity & engagement | 12 | 12 | 0 | 0 |
| Delivery & tone | 10 | 9 | 0 | 1 |
| Generative & assistant | 14 | 11 | 1 | 2 |
| Originality & provenance | 8 | 3 | 3 | 2 |
| Personalization & team | 12 | 11 | 0 | 1 |
| Platform & UX | 13 | 11 | 1 | 1 |
| Languages | 3 | 3 | 0 | 0 |
| Enterprise & security | 6 | 0 | 2 | 3 |
| Voice learning | 6 | 6 | 0 | 0 |
| **Total** | **94** | **76** | **7** | **10** |

(One additional enterprise feature, BYOK encryption, is marked `N/A` because our BYOK is an API key, not an encryption key.)

The point is not the count. It is that **76 of 94 features are reachable at good-or-full parity**, while the 10 omitted features are exactly the data-licensing/enterprise businesses that a small open-source project should not enter.

---

## 1. Correctness

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Spelling | Free | Full | Harper local + personal dictionary + LLM confirmation | M1 | S | Low |
| 2 | Grammar | Free | Full | Harper local + LLM structured edits | M1 | M | Medium |
| 3 | Punctuation | Free | Full | Harper local + LLM | M1 | S | Low |
| 4 | Capitalization | Free | Full | Harper local + LLM | M1 | S | Low |
| 5 | Commonly confused words | Free | Full | Harper rules + LLM | M1 | S | Low |
| 6 | Formatting hygiene (double spaces, doubled words) | Free | Full | Local regex rules | M1 | S | Low |
| 7 | Sentence structure (fragments, run-ons) | Free/Pro | Full | LLM | M1 | M | Medium |
| 8 | Dialect consistency (US/UK/CA/AU/IN) | Free | Full | Harper dialect + prompt | M1 | S | Low |
| 9 | Multilingual correctness (24 languages) | Free | Good | LLM; local rules English-only | M3 | M | Medium |
| 10 | Add to dictionary | Free | Full | SQLite + Harper dictionary + `NSSpellChecker`-style ignore | M1 | S | Low |

## 2. Clarity and engagement

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 11 | Conciseness | Pro | Full | LLM minimal-edit prompt | M1 | M | Medium |
| 12 | Wordiness / redundancy | Pro | Full | LLM + local phrase rules | M1 | M | Medium |
| 13 | Clarity / tangled sentences | Pro | Full | LLM | M1 | M | Medium |
| 14 | Passive voice flag | Pro | Good | Local heuristic + LLM rewrite on request | M2 | M | Medium (false positives) |
| 15 | Readability score | Pro | Full | Local Flesch-Kincaid/Gunning Fog | M2 | S | Low |
| 16 | Hedging / vague language | Pro | Good | Lexicon + LLM | M2 | M | Medium |
| 17 | Vocabulary / word choice | Pro | Good | LLM, opt-in only | M2 | M | High (false positives) |
| 18 | Sentence variety | Pro | Good | Local statistics + LLM | M2 | M | Medium |
| 19 | Repetitive words/phrases | Pro | Full | Local n-gram analysis | M1 | S | Low |
| 20 | Compelling language / weak verbs | Pro | Good | LLM | M2 | M | Medium |
| 21 | Jargon / technical terms | Pro | Good | Style rules + LLM | M4 | M | Medium |
| 22 | Full-sentence rewrite | Pro | Full | LLM + diff | M2 | M | Medium |

## 3. Delivery and tone

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 23 | Tone detection | Free | Full | LLM classification, 150–2,000 chars, cached | M1 | M | Medium |
| 24 | Tone adjustment | Pro | Full | LLM rewrite with tone params | M2 | M | Medium |
| 25 | Formality dial | Pro | Full | LLM | M2 | S | Low |
| 26 | Confidence dial | Pro | Full | LLM | M2 | S | Low |
| 27 | Friendliness dial | Pro | Full | LLM | M2 | S | Low |
| 28 | Inclusive language | Pro | Good | Curated lexicon + LLM | M2 | M | Medium |
| 29 | Paragraph rewrite | Pro | Full | LLM + diff + accept/reject | M2 | L | Medium |
| 30 | Brand tones | Pro (1) / Enterprise | Good | Few-shot voice profile from samples | M4 | L | Medium |
| 31 | Reader Reactions | Pro | Good | LLM role-play + rubric | M4 | M | Medium |
| 32 | Strategic suggestions | Enterprise | Omit | Out of scope for v1 | Never | — | — |

## 4. Generative and assistant

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 33 | Compose / draft from prompt | Free 100/mo; Pro 2,000 | Full | LLM + insert/replace | M3 | M | Medium |
| 34 | Reply in context | Pro/Go | Full | Context capture + LLM | M3 | L | High (context) |
| 35 | Summarize | Pro | Full | LLM map-reduce | M3 | M | Medium |
| 36 | Rewrite with custom instruction | Pro | Full | LLM | M2 | S | Low |
| 37 | AI Chat side panel | Free/Pro | Good | Assistant panel + context chips | M3 | L | Medium |
| 38 | Suggested prompts | Free | Good | Static + context-aware chips | M3 | S | Low |
| 39 | Ask / translate / explain selection | Go | Full | Selection actions + LLM | M3 | M | Low |
| 40 | Humanizer | Pro/Go | Full | Style rewrite prompt; acceptable-use policy | M3 | S | Medium (misuse) |
| 41 | AI Rewriter (AI-phrase flags) | Pro | Good | Local AI-cliché lexicon + LLM | M4 | S | Low |
| 42 | Paraphraser modes | Paid | Full | LLM presets | M3 | S | Low |
| 43 | Knowledge Checker | Enterprise | Substitute | RAG over user-provided docs | Later | L | Medium |
| 44 | AI Grader | Education | Omit | Out of scope | Never | — | — |
| 45 | Resume builder | Paid | Omit | Out of scope | Never | — | — |
| 46 | Citation generator / finder | Free/Pro | Good | LLM + Crossref/OpenAlex | Later | M | Medium |

## 5. Originality, authenticity, provenance

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 47 | Plagiarism checker | Pro | Substitute | Local MinHash vs user corpora + optional web search; label "similarity check" | Later | XL | High |
| 48 | AI detector | Pro | Omit | No verdict; optional third-party via BYOK; AI-phrase linting instead | Never | — | — |
| 49 | AI-phrase / AI-ish prose linting | Pro (AI Rewriter) | Good | Lexicon + heuristics, framed as style feedback | M4 | S | Low |
| 50 | Authorship / provenance | Free basic/Pro | Good | Local append-only log: typed/pasted/AI edits + report | M4 | L | Medium |
| 51 | Citation formatting | Free basic/Pro | Good | LLM + Crossref | Later | S | Low |
| 52 | Citation finder | Pro | Substitute | Semantic Scholar/OpenAlex search | Later | M | Medium |
| 53 | Knowledge Share | Pro | Substitute | Local glossary + hover + LLM context | M4 | M | Medium |
| 54 | Expert Review / human editing | Discontinued | Omit | — | Never | — | — |

## 6. Personalization and team

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 55 | Personal dictionary | Free | Full | SQLite + Harper | M1 | S | Low |
| 56 | Organizational dictionary | Pro | Full | CSV/YAML import | M4 | S | Low |
| 57 | Style guide | Pro (1) / Enterprise | Good | Vale-style YAML rules + RAG over uploaded guide | M4 | L | Medium |
| 58 | Snippets | Pro/Enterprise | Full | Local snippet library + trigger picker | M4 | M | Low |
| 59 | Advanced snippets (inputs/dropdowns) | Enterprise/Education | Good | Template variables + date/time | Later | M | Low |
| 60 | Analytics (team) | Pro/Enterprise | Omit | No surveillance/team dashboards | Never | — | — |
| 61 | Local personal analytics | Pro | Good | Local accept rate/cost/category dashboard | M4 | M | Low |
| 62 | Communication score | Paid | Good | Local rubric score with breakdown | M4 | M | Medium |
| 63 | Docs editor | Free/Pro | Good | Local rich-text editor + AI actions | M4 | L | Medium |
| 64 | Suggestion cards | Free | Full | Core UI | M1 | M | Low |
| 65 | Per-app preferences | Free | Full | SQLite app rules | M2 | S | Low |
| 66 | Natural-language preferences | New 2026 | Good | "Only show grammar and clarity" → parsed settings/prompt | M4 | M | Medium |

## 7. Platform and UX

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 67 | Menu-bar app | Free | Full | Tauri tray / LSUIElement | M0 | M | Low |
| 68 | Floating widget | Free | Full | Non-activating panel, per-app anchor | M2 | L | High (focus) |
| 69 | Inline underlines | Free/Pro | Full | AX bounds + click-through overlay (level 1500) | M2 | L | High (geometry) |
| 70 | Right-edge assistant tab | Free/Pro | Good | Tauri panel, tabs | M3 | L | Medium |
| 71 | Selection actions (blue sidebar) | Pro | Good | Floating action bar on selection | M2 | M | Medium |
| 72 | Per-app block list / off for 1 hour | Free | Full | App rules + timer | M1 | S | Low |
| 73 | Keyboard shortcuts | Free | Full | Carbon hotkeys, configurable | M1 | S | Low |
| 74 | Login item | Free | Full | SMAppService/Tauri autostart | M0 | S | Low |
| 75 | Auto-update | Free | Full | Tauri updater, signed | M5 | M | Medium |
| 76 | Browser extension (Chrome/Edge) | Free | Full | MV3 + local bridge | M3 | XL | High |
| 77 | Safari extension | Free | Good | Safari Web Extension, same notarized app | Later | L | Medium |
| 78 | Google Docs support | Free | Omit | Canvas; explicit unsupported message | Never | — | — |
| 79 | OCR fallback for unsupported apps | — | Substitute | ScreenCaptureKit + Vision, opt-in only | Later | L | High (permissions) |

## 8. Languages

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 80 | Suggestions in 24 languages | Free | Good | LLM; disable local rules for non-English | M3 | M | Medium |
| 81 | Paragraph rewrites in 6 languages | Pro | Good | LLM | M3 | S | Low |
| 82 | Translation (19 languages) | Pro | Full | LLM | M3 | S | Low |

## 9. Enterprise and security

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 83 | SSO/SAML | Enterprise | Omit | No accounts; local app | Never | — | — |
| 84 | SCIM | Enterprise | Omit | No accounts | Never | — | — |
| 85 | DLP / confidential mode | Enterprise | Substitute | Local-only mode + per-app rules + no retention | M1 | S | Low |
| 86 | Audit log API | Enterprise | Substitute | Local exportable request/provenance log | M4 | M | Low |
| 87 | BYOK encryption | Enterprise | N/A | We use BYOK API keys; encryption at rest via FileVault/Keychain | M1 | — | — |
| 88 | Custom roles/permissions | Enterprise | Omit | Out of scope | Never | — | — |

---

## 10. Voice learning

| # | Feature | Grammarly tier | Target | Approach | Phase | Effort | Risk |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 89 | Voice profile learned from accepted/dismissed/edited suggestions (traits, rhetorical moves, signature phrases) | No direct equivalent | Full | Local sufficient statistics + weighted feedback | M2 | M | Medium |
| 90 | Upload writing samples (txt/md/docx/paste) | Brand tones (paid) | Full | Local parsing, quality gate, feature extraction, exemplars | M3 | L | Medium |
| 91 | Per-context + multiple voice profiles | Partial (brand tones) | Full | Context detection + profile mapping; Personal/Work/Brand | M4 | M | Medium |
| 92 | Voice preview + "sounds like me" rating | No direct equivalent | Full | Generate a sample rewrite; rating updates the profile | M2 | S | Low |
| 93 | Voice strength, style-card-only, freeze/reset | No direct equivalent | Full | Off/light/balanced/strong; payload preview; profile controls | M2–M4 | S | Low |
| 94 | Voice export/import + encrypted samples | No direct equivalent | Full | JSON export/import; AES-256-GCM sample storage; Keychain-wrapped key | M4 | M | Medium |

**Why this matters:** Grammarly's personalization is cloud-side and brand-tone-oriented. Harper has no LLM, so no voice-aware rewriting. A local, explainable, provider-agnostic voice profile is a genuine differentiator — and it is the feature that makes "your model" feel like *your* writing rather than a generic assistant.

**Guardrails:** off by default; samples encrypted and deletable; excerpts previewed; 8-gram copy guard; AI-accepted text down-weighted; no sensitive-attribute inference; no impersonation.

---

## 11. Platform coverage

● v1/M3 target · ○ planned · — not applicable

| Capability | macOS | Chrome/Edge | Windows | iOS | Android |
| --- | :---: | :---: | :---: | :---: | :---: |
| System-wide inline suggestions | ● | ● | ○ | — | ○ |
| Menu bar / system tray | ● | — | ○ | — | — |
| Browser editor support (Gmail, Notion, Slack) | via extension | ● | via extension | — | — |
| Google Docs | — | — | — | — | — |
| Voice profile (learn + apply) | ● | ● | ○ | ○ | ○ |
| Assistant / compose / reply | ● | ● | ○ | ○ | ○ |
| BYOK providers | ● | via app | ○ | ○ | ○ |
| Local-only mode | ● | via app | ○ | ○ | ○ |
| Style guide / snippets / glossary | ● | ● | ○ | ○ | ○ |
| System-wide text capture | ● | — | ○ | keyboard only | ○ |
| Keyboard extension | — | — | — | ○ | ○ |
| Background menu-bar agent | ● | — | ○ | — | — |

The shared Rust core makes these rows reuse the same analysis, provider, voice, and storage code; only the platform shell changes. See [`apps/README.md`](../apps/README.md) and [ADR-021](DECISIONS.md).

---

## Critical path

These features determine whether the product is credible. Everything else can slip.

1. **Stable capture + injection** (M0–M1): read the right field, replace the right span, verify, never steal focus.
2. **Offset resolution** (M1): quoted spans → exact/normalized/fuzzy → drop ambiguous. This is the quality moat.
3. **Local + LLM merge** (M1–M2): instant correctness, debounced judgment, no duplicate/conflicting underlines.
4. **Suggestion UX** (M1–M2): card, widget, diff, accept/dismiss, explanation.
5. **Voice profile** (M2–M4): style card, feedback learning, uploads, per-context voices, copy guard. The second differentiator after model choice.
6. **Browser extension** (M3): Gmail/Notion/Slack/Linear; Google Docs explicitly unsupported.
7. **Provider flexibility** (M3): OpenRouter, OpenAI-compatible, Anthropic, Gemini, Ollama.
8. **Privacy/transparency** (M1–M4): local-only mode, network log, cost meter, no telemetry.
9. **Signed/notarized release + updates** (M5): without this, TCC re-prompts and the product is not installable.

## Deferred deliberately

- Google Docs (canvas), Safari extension, OCR, native messaging, docs editor, web similarity, citations, Windows/iOS/Android (post-v1), enterprise admin, local LoRA fine-tuning.
- Each is valuable; none is on the path to a credible macOS v1.
