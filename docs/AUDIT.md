# Product Audit — Grammarly / Superhuman in 2026

**Prepared:** 19 October 2026
**Purpose:** decide what to build, what to copy, what to avoid, and how to differentiate an open-source, bring-your-own-key (BYOK) macOS writing assistant.
**Method:** direct inspection of Grammarly/Superhuman public pages, the Grammarly Help Center (public Zendesk API), GitHub API metadata and source for open-source comparables, Apple platform documentation, and the live OpenRouter model catalog. Search-engine corroboration was unavailable in this environment; vendor claims are labelled and pricing is as rendered on 19 October 2026. Full research notes: [`research/grammarly-audit.md`](../research/grammarly-audit.md).

---

## 0. Executive summary

### The five findings that change the plan

1. **The competitor is Superhuman, not Grammarly.** Grammarly acquired Superhuman in 2025; in 2026 the parent company rebranded as **Superhuman** and launched **Superhuman Go**, an agent platform. Grammarly is now one agent inside Go. Enterprise is sold "through Superhuman Go." Superhuman also agreed to acquire **GPTZero** (AI detection/plagiarism) on 23 June 2026. A positioning doc that says "open-source Grammarly" is describing the 2023 product.

2. **Grammarly's architecture is the wedge.** There is **no documented offline mode**. Its clients are thin: text is analyzed in Grammarly's cloud, and the privacy documentation points to AWS us-east. Enterprise "BYOK" means **bring-your-own-encryption-key**, not bring-your-own-model-API-key; you can never supply your own model. "Your text never leaves the Mac, and the model is your key" is a claim Grammarly cannot copy without cannibalizing its business. *(The offline claim is an architectural inference from the client design and the lack of any offline documentation; treat it as high-confidence, not vendor-confirmed.)*

3. **The paywall is at judgment, not correctness.** Free gets red correctness underlines, tone *detection*, and 100 AI prompts/month. Pro ($12/mo billed annually, $30 monthly) gets blue clarity/engagement/delivery underlines, sentence and paragraph rewrites, tone adjustment, plagiarism, AI detection, and 2,000 prompts. The features we can deliver as one prompt each — humanizer, translation (19 languages), multilingual suggestions (24 languages), tone adjustment, paragraph rewrites — are exactly what Grammarly charges for.

4. **Harper Desktop already occupies "private, free, system-wide macOS grammar."** It is Apache-2.0, free, no account, no telemetry, macOS 14+, with a system-wide Accessibility overlay, per-app enable, personal dictionary, five dialects, custom rules ("Weirpacks"), and sub-10ms local checks. It explicitly has **no LLM in the loop**. Automattic-backed, ~16.7k stars, v2.13.0 (16 Oct 2026), currently beta. We should not rebuild this. The empty intersection is **Harper's local-first macOS ergonomics + Grammarly's paid-tier rewriting quality + the user's own key.**

5. **The macOS mechanism is settled.** It is an Accessibility-API overlay, not an input-method hack. Grammarly's own Jamf deployment guide requires an MDM configuration profile to pre-grant Accessibility; Harper Desktop and Kerlig use the same approach. Budget for the permanent tax: per-app breakage, canvas editors, secure fields, OS updates.

### Recommendation in one sentence

**Fork or vendor Harper Desktop's Apache-2.0 macOS shell and local rule engine, rebrand it, and build the missing layer: BYOK providers, LLM-powered judgment (clarity, tone, rewrites, compose, translate, humanize), a Grammarly-quality suggestion UX, a browser extension, and radical privacy/cost transparency.**

### What "parity" realistically means

| Layer | Parity | Notes |
| --- | --- | --- |
| Correctness (spelling, grammar, punctuation, capitalization) | **High** | Harper local rules + LLM confirmation; English local, 20+ languages via LLM |
| Clarity, conciseness, engagement, tone | **High** | LLM structured edits with local validation |
| Rewrites, tone adjustment, compose/reply/summarize, translation, humanizer | **High** | One prompt each; quality tracks the chosen model |
| Style guide, brand tones, glossary, snippets, dictionary | **High** | YAML rules + RAG + local glossary; no admin console |
| Voice learning (personal tone/nuances) | **High** | Local style card + feedback + exemplars; no cloud fine-tuning; provider-agnostic |
| Inline UX (underlines, widget, cards, assistant panel) | **Medium–High** | Harper plumbing + our UI; per-app matrix |
| Browser coverage (Gmail, Notion, Slack, Linear) | **High** | MV3 extension required; Google Docs canvas is explicitly out of scope in v1 |
| Plagiarism | **Low** | No corpus; substitute local corpus overlap + web search + citations |
| AI detection | **Low** | No training data; substitute AI-phrase linting + provenance; do not ship a verdict |
| Enterprise admin (SSO, SCIM, DLP, audit) | **Low** | Out of scope; local policy files/MDM instead |
| App coverage claim ("1M+") | **N/A** | Publish a tested matrix, not a number |

---

## 1. Corporate and product map

| Year | Event |
| --- | --- |
| 2024 | Grammarly acquires Coda; Coda CEO Shishir Mehrotra becomes Grammarly CEO |
| 2025 | Grammarly agrees to acquire Superhuman (email client) |
| 2026 | Parent company rebrands as **Superhuman**; launches **Superhuman Suite** and **Superhuman Go** |
| 23 Jun 2026 | Superhuman agrees to acquire **GPTZero** (AI detection, plagiarism, hallucination detection) |

Current product architecture:

| Product | What it is | Grammarly's role |
| --- | --- | --- |
| **Superhuman Go** | Proactive AI assistant + agent platform; "works in 1M+ apps"; agents for writing, email, knowledge, daily brief, humanizer, AI detector; custom agents; Agent Store; MCP connectors | Grammarly is one agent inside Go |
| **Grammarly** | Consumer writing product, still sold at grammarly.com | Free/$12 Pro writing assistant |
| **Superhuman Docs** | AI-native docs/collaboration (Coda lineage) | Absorbs the old Grammarly Editor/Docs |
| **Superhuman Mail** | Email client | Formerly Superhuman Mail |

Practical naming:

- grammarly.com still sells **Grammarly Free** and **Grammarly Pro** at consumer prices; the brand is alive at the individual tier.
- Enterprise is sold as **Superhuman Go**.
- Help Center: "Grammarly is now part of Superhuman, a suite of AI tools that includes Superhuman Go, Grammarly, Coda, and Superhuman Mail."
- The desktop client is being renamed: "Grammarly for Windows and Mac" and "Superhuman Go for Windows and Mac" are separate apps that cannot run at the same time.

**Implication:** our competitor is an agent platform whose wedge is proactive assistance in every app. The grammar checker is commoditized. Our differentiation must be **private, key-owned, high-quality rewriting** and **user control**, not "better underlines."

---

## 2. Surfaces

| Surface | Platforms | Notes |
| --- | --- | --- |
| Browser extension | Chrome, Edge, Safari, Firefox | Inline suggestions on 1M+ sites; required for Google Docs |
| Desktop app | Windows, macOS | Accessibility overlay, floating widget + right-edge tab; menu-bar app |
| Superhuman Go app | Win, macOS, Chrome/Edge, iOS/Android, web | Successor client; mutually exclusive with Grammarly desktop |
| Web editor / Docs | Any browser | app.grammarly.com; Docs at docs.superhuman.com |
| Mobile | iOS/iPadOS/Android + Grammarly Keyboard | Out of scope for a macOS-first project |
| MS Word/Outlook | Via desktop app | Word-on-Mac add-in discontinued; Office add-in retiring |
| Google Docs | Browser extension only | Desktop app cannot hook the canvas |
| Linux | Web/extension only | No native app |

Discontinued and worth ignoring: Grammarly for Microsoft Word on Mac (discontinued), Office add-in (retiring), expert human writing service (ended Jan 2025), Expert Review (discontinued March 2026).

---

## 3. Feature inventory

Tier legend: **F** Free, **P** Pro, **E** Enterprise (via Superhuman Go), **Ed** Education. "Cloud" is **Yes** for every Grammarly feature — the clients are thin; analysis happens on Grammarly's servers.

### 3.1 Correctness

| Feature | Description | Tier | Cloud | Our approach | Parity |
| --- | --- | --- | --- | --- | --- |
| Grammar | Agreement, tense, articles, prepositions, fragments, run-ons | F | Yes | Harper local + LLM structured edits | High |
| Spelling | Typos, misspellings, add to dictionary | F | Yes | Harper local + personal dictionary | High |
| Punctuation | Commas, dashes, apostrophes, quotes, Oxford comma | F | Yes | Harper local + LLM | High |
| Capitalization | Sentence starts, proper nouns, titles | F | Yes | Harper local + LLM | High |
| Commonly confused words | their/there, affect/effect, less/fewer | F | Yes | Harper rules + LLM | High |
| Formatting hygiene | Double spaces, doubled words, mismatched quotes/parens | F | Yes | Local rules | High |
| Dialect | US/UK/CA/AU/IN English | F | Yes | Harper dialect setting + prompt | High |
| Multilingual correctness | Suggestions in 24 languages | F | Yes | LLM; local rules English-only | High (major languages) |

Free-tier behavior: correctness issues are **red underlines**, inline, without a sidebar.

### 3.2 Clarity, engagement, delivery

| Feature | Tier | Our approach | Parity |
| --- | --- | --- | --- |
| Conciseness / wordiness | P | LLM minimal-edit prompt | High |
| Readability / sentence structure | P | Local formulas + LLM | High |
| Passive voice | P | Local heuristic flag + LLM rewrite | Medium |
| Hedging / vague language | P | Lexicon + LLM | Medium |
| Full-sentence rewrite | P | LLM + diff preview | High |
| Paragraph rewrite | P | LLM + diff preview | High |
| Vocabulary / word choice | P | LLM, opt-in (false-positive risk) | Medium |
| Sentence variety | P | Local statistics + LLM | Medium |
| Repetitive words | P | Local n-gram analysis | High |
| Compelling language / weak verbs | P | LLM | Medium |
| Tone detection | F | LLM classification + cache; 150–2,000 chars | High |
| Tone adjustment | P | LLM rewrite with tone parameters | High |
| Formality / confidence / friendliness | P | LLM dials | High |
| Inclusive language | P | Curated lexicon + LLM | Medium |
| Brand tones | P (1) / E (unlimited) | Named voice profiles with sample documents | High |
| Reader Reactions | P | LLM role-play with rubric | Medium |
| Strategic suggestions | E | LLM audience analysis | Low priority |
| Translation | P | LLM; 19 languages, 4,000-char limit in Grammarly | High |

Clarity/engagement/delivery issues are **blue underlines** and are explicitly paid.

### 3.3 Generative and agentic

| Feature | Tier | Our approach | Parity |
| --- | --- | --- | --- |
| Prompt-based generation | F 100/mo; P 2,000/mo; E unlimited | LLM; no artificial quota | High |
| Compose / draft | F limited | LLM + insert | High |
| Reply in context | P | Context capture + LLM | High (context is the hard part) |
| Summarize | P | LLM map-reduce | High |
| Rewrite with custom instruction | P | LLM | High |
| AI Chat side panel | F/P | Assistant panel with context chips | Medium–High |
| Suggested prompts | F | Context-aware prompt chips | Medium |
| Ask / translate / explain selection | Go extension | Selection actions | High |
| Humanizer | P | Style rewrite prompt | High (easy) |
| AI Rewriter (AI-phrase flags) | P | Local AI-cliché lexicon + optional LLM | Medium |
| Knowledge Checker | E | RAG over user-provided docs | Low priority |
| AI Grader | Ed | LLM rubric | Out of scope |
| Resume builder / Citation Finder | P/Ed | Templates + Crossref/OpenAlex | Low priority |

### 3.4 Originality, authenticity, and provenance

| Feature | Tier | Reality | Our approach |
| --- | --- | --- | --- |
| Plagiarism checker | P | "Billions of web pages" + licensed academic corpora; desktop only in Word | **No corpus.** Local MinHash against user-supplied corpora + optional web search API + citation lookup. Label "similarity check," never "plagiarism guarantee." |
| AI detector | P | Proprietary classifier; vendor claims "99% accuracy / #1 RAID"; GPTZero acquisition | **Do not ship a verdict.** Optional third-party detector via BYOK; local AI-phrase linting as style feedback. |
| Authorship | F basic / P detailed | Clipboard + cloud reports; capped at 20 pages in Word | **Local provenance log** (typing, paste hashes, AI edits) + exportable report. More honest, fully local, education differentiator. |
| Citation generator / finder | F/P | Cloud + citation databases | LLM formatting + Crossref/OpenAlex/Semantic Scholar (free APIs) |
| AI Grader | Ed | Rubric grading | Out of scope |

### 3.5 Documents, knowledge, team

| Feature | Tier | Our approach | Parity |
| --- | --- | --- | --- |
| Docs editor | F/P | Local rich-text editor (or defer) | Medium |
| Suggestion cards | F | Core UI: change, explanation, accept/dismiss/more | High |
| Knowledge Share | P | Local glossary + hover tooltips + LLM context | Medium |
| Snippets | P/E | Local snippet library with trigger/picker | High |
| Personal dictionary | F | Local dictionary shared with local rules | High |
| Organizational dictionary | P | CSV/YAML import | High |
| Style guide | P (1) / E (unlimited) | Vale-style YAML + RAG over uploaded guide | High |
| Analytics | P/E | Local-only personal dashboard; no surveillance | Medium |
| Feature access / admin | E | Out of scope | Low |

### 3.6 The 2026 agent layer (important for positioning)

Superhuman Go is a platform:

- **Proactive suggestions** annotate as you type, everywhere.
- **Agent team**: Grammarly, Email Assistant, Knowledge Checker, Daily Brief, Humanizer, AI Detector; custom agents triggered while writing, on a schedule, or on an event.
- **Agent Store**: 100+ agents.
- **Connectors**: MCP and pre-built connectors.
- **Surfaces**: inline underline, side-panel chat, on-cursor actions, Slack @-mentions, Go app, Docs.
- **Natural-language customization**: "Tell Grammarly what edits you find useful, and it'll only show you the underlines that matter." Users can type preferences ("Write AM and PM in uppercase") saved as agent settings.

This shift matters: fixed suggestion categories are being replaced by **prompt-steerable preferences**. We should ship **preferences-as-natural-language** from day one — cheaper than 40 toggles and where the incumbent is heading.

### 3.7 Language coverage

- Suggestions: **24 languages** (Spanish, French, Italian, German, Portuguese, Turkish, Polish, Dutch, Czech, Ukrainian, Vietnamese, Hungarian, Swedish, Romanian, Indonesian, Slovak, Danish, Finnish, Norwegian, Tagalog, Korean, Hindi, Japanese, Chinese).
- Paragraph rewrites: Spanish, French, Italian, German, Portuguese, Ukrainian.
- Translation: 19 languages, 4,000-character limit in Grammarly.
- Five English dialects: US, UK, CA, AU, IN.

For a BYOK app, multilingual quality falls out of the LLM. Local rules stay English (Harper) unless we add LanguageTool later.

---

## 4. macOS desktop teardown

### 4.1 What it installs

From the Jamf deployment guide:

- `.dmg` from `download-mac.grammarly.com`; volume "Grammarly".
- Installs `Grammarly Desktop.app` to `/Applications` (or `~/Applications`).
- Bundle identifier: `com.grammarly.ProjectLlama`.
- Quarantine stripped by MDM; first-run launched with `--args launchSourceInstaller`; onboarding can be deferred.
- Login item; menu-bar icon; no root LaunchDaemon.
- **Accessibility permission is mandatory and pre-grantable by MDM configuration profile.**
- Requires a browser engine present: Chrome 89+ or Safari 12.1+.
- System requirements: macOS 11+ for Go; 2018+ Mac; consumer page still says macOS 10.15+ (stale).

### 4.2 How it runs

- Menu-bar app, always resident; launching from `/Applications` restores the icon.
- Grammarly desktop and Go desktop are mutually exclusive.
- Per-app block list; "turn off for 1 hour"; "turn off for this app/site."
- Settings: dialect, tone detection toggle, suggestion categories, per-app widget position.

### 4.3 How suggestions are surfaced

1. **Inline underlines** drawn over the host app's text, aligned to Accessibility text geometry.
2. **Right-edge vertical tab / side panel** with Review suggestions, agent list, and chat grounded in the active window.
3. **Floating widget** (legacy): draggable, magnetic anchor points around text fields, suggestion count, tone emoji, per-app position.
4. **Selection actions**: highlight text → blue sidebar on the left with shorten/formalize/translate/custom prompt → Accept.
5. **Suggestion cards** on hover: change, explanation, Accept/Dismiss/three-dot menu.

### 4.4 Shortcuts (macOS)

| Action | Shortcut |
| --- | --- |
| Open assistant | `⇧⌃⌘G` |
| Open AI Chat | `⌃G` |
| Toggle sidebar | `⇧⌘G` |
| Snippets picker | `\` |
| Create snippet | `⌃⇧S` |

The shortcut namespace is crowded. Pick a collision-free chord and make it configurable from day one.

### 4.5 Supported apps and limitations

Grammarly's own list: Word, Outlook, PowerPoint, Teams, Pages, Notes, Slack, Discord, Apple Mail, Gmail, Google Docs, Sheets, Confluence, Jira, Salesforce, HubSpot, Zendesk, LinkedIn, WordPress, Medium, YouTube, X, Facebook, Instagram, Yahoo. "1M+ apps" is a claim about any standard text field, not verified integrations.

Vendor-documented limitations we would inherit by copying the overlay:

| Limitation | Detail |
| --- | --- |
| Plagiarism + AI detection only in Word on Mac | Not Mail, Slack, Docs, Pages |
| Google Docs needs the extension | Desktop app cannot hook the canvas |
| Gmail subject line not checked | Documented gap |
| Docs break with tables/footnotes | Inline underlines fail |
| Docs slow past 30 pages | Performance cliff |
| PowerPoint on Mac malfunctions | Vendor-documented |
| Authorship capped at 20 pages in Word | History unreliable past cap |
| Only one client active | Grammarly and Go cannot run together |
| Electron apps | Work, but depend on their AX tree |
| Secure input fields | macOS Secure Event Input blocks reading; expect no suggestions in passwords/cards |
| No documented offline mode | Clients are thin; analysis runs in Grammarly's cloud |

**Honest read:** the "1M+ apps" claim is true for well-behaved text fields and progressively less true for canvas editors, long documents, secure fields, and OS changes. Publish a tested matrix.

---

## 5. Pricing and packaging

| Plan | Price | Includes |
| --- | --- | --- |
| Free | $0 | Correctness (red underlines), tone detection, 100 AI prompts/month |
| Pro (annual) | $12/member/mo, billed annually | Blue clarity/engagement/delivery underlines, sentence + paragraph rewrites, tone adjustment, plagiarism, AI detection, unlimited personalized suggestions, 2,000 prompts/month; 1–149 seats |
| Pro (monthly) | $30/member/mo | Same |
| Premium (App Store legacy) | App-store pricing | Core writing features, 1,000 prompts/month, no team features |
| Enterprise (via Superhuman Go) | Contact sales | Pro + proactive AI everywhere, unlimited prompts/members, dedicated support, BYOK **encryption**, custom roles, DLP, SSO/SAML, SCIM, audit API, confidential mode, cost centers |
| Education | Contact sales | Authorship, AI Grader, LMS; 3,000+ institutions |

Notes:

- **Pro annual is $12/mo; monthly is $30/mo.** The audit brief's "$12/month" is the annual-billing rate.
- Prompt limits are inconsistent: plans page says 100/2,000/unlimited; the Go desktop app reportedly allows 1,000 prompts/day.
- Enterprise "BYOK" = bring-your-own-encryption-key, not bring-your-own-API-key. Grammarly never lets you supply a model.
- The paywall is at **judgment** (clarity, tone, rewrites), not correctness. That is exactly where an LLM-native competitor can give away value.

### Consumer cost comparison (illustrative)

Using October 2026 OpenRouter list prices for a 150-word email (~600 input tokens including prompt, ~250 output):

| Model class | Example | Cost per review | 1,000 reviews |
| --- | --- | --- | --- |
| Ultra-cheap | `deepseek/deepseek-v4.1-flash` ($0.034/$0.08 per 1M) | ~$0.00004 | ~$0.04 |
| Cheap fast | `anthropic/claude-haiku-5.5` ($0.10/$0.50) | ~$0.0002 | ~$0.19 |
| Mid | `google/gemini-3.8-flash` ($0.825/$4.125) | ~$0.0015 | ~$1.50 |
| Frontier | `anthropic/claude-sonnet-5.5` ($2/$10) | ~$0.0037 | ~$3.70 |

A 1,500-word document costs roughly 4–8× that per review. The economics favor a cheap default with an explicit "use a stronger model" action.

---

## 6. What is hard, moot, or a bad idea to copy

| Capability | Why not replicate | Realistic substitute |
| --- | --- | --- |
| **Plagiarism corpus** | Web-scale crawl + licensed academic/professional corpora; permanent infrastructure/legal cost | Local MinHash overlap against user-supplied corpora; quote/attribution hygiene; optional web search API with clear labeling |
| **AI detector** | Proprietary labeled data + continual adversarial retraining; even leaders are contested; harmful false accusations | AI-phrase linting as style feedback; optional third-party detector via BYOK; provenance instead of verdict |
| **Humanizer** | Actually easy with an LLM; Grammarly's edge is evaluation | Ship it as a style preset; do not market detector evasion |
| **Authorship** | Grammarly uses clipboard + cloud | Local append-only provenance log (typing, paste hashes, AI edits) + report; better and private |
| **Enterprise style guide "ML"** | Aggregate behavioral data from 50k orgs | Vale-style YAML rules + RAG over the uploaded guide; deterministic, auditable, 80% of value |
| **Brand tones (learned)** | Trained on org writing samples | Few-shot voice profile from 3–10 sample documents, stored locally |
| **Knowledge Share** | Company term DB + admin + per-app hover | Local glossary + hover + LLM context, one surface first |
| **"1M+ apps"** | It's the Accessibility API, fragile per app | Copy the mechanism, publish a tested matrix, degrade gracefully |
| **AI Grader / Education suite** | Rubric modeling, LMS integrations, FERPA compliance, institutional sales | Out of scope |
| **Real-time latency at scale** | Datacenter models + 17-year incremental engine | **Hybrid:** local rules every keystroke (<10ms), LLM only on demand/debounced |
| **Multilingual rule packs** | Years of language-specific work | LLM handles 20+ languages; local rules English-only initially |
| **SOC 2 / ISO / HIPAA / DLP / SCIM** | Years of audit work | Turn local-first into the advantage: no text leaves the device; smaller breach surface; simple data statement |
| **Expert human editing** | Grammarly killed it | Don't build a marketplace |

Also moot: copying the fixed suggestion-category taxonomy. Grammarly is moving to prompt-steerable preferences. Ship natural-language preferences.

---

## 7. Competitive landscape

### 7.1 Open source and self-hosted

| Project | License | Approach | macOS | BYOK/model | Activity | Lesson |
| --- | --- | --- | --- | --- | --- | --- |
| **Harper** | Apache-2.0 | Rule-based Rust, local, English | **Harper Desktop**, system-wide overlay, macOS 14+, beta | No LLM at all | v2.13.0, ~16.7k★, very active | **Direct prior art and potential engine.** Study/fork; don't rebuild the plumbing |
| **LanguageTool** | LGPL-2.1 | Rules + n-grams, 25+ languages, Java | Java app/LSP, not native overlay | BYO server (self-host), not BYO LLM key | Very active, ~15.2k★ | Local-server + thin-client works; heavy memory |
| **Vale** | MIT | Prose linter, markup-aware, Go | CLI | N/A | Active, ~6.3k★ | Style guides as versioned YAML |
| **textlint** | MIT | Pluggable JS linter | CLI/Node | N/A | Active, ~3.2k★ | Plugin ecosystem |
| **LTeX+** | MPL-2.0 | LSP wrapper over LanguageTool | Editor LSP | BYO LanguageTool server | Active | Language-server packaging |
| **proselint** | BSD-3 | Style/usage linter | CLI | N/A | Maintained, ~4.6k★ | Curated rule corpus is cheap and high-signal |
| **write-good / alex** | MIT | Naive linters | CLI | N/A | Stale | Cautionary tales: naive heuristics produce false positives |

### 7.2 Commercial and platform

| Product | Model | macOS | BYOK | Notes |
| --- | --- | --- | --- | --- |
| **Grammarly / Superhuman** | Closed subscription | Mature | No (enterprise encryption key only) | Cloud-only; corpus; brand; 1M+ app claim |
| **Apple Writing Tools** | Free with OS | System-wide, on-demand | No | On-device/Private Cloud Compute; not proactive; no custom rules; Apple models only |
| **Harper Desktop** | Free, Apache-2.0 | System-wide overlay | No (no LLM) | Local, <10ms, per-app, custom rules; English-only; no rewriting |
| **Kerlig** | Commercial BYOK writing app | Yes | **Yes**, 350+ models | Closest commercial analogue; selection-triggered; built-in/custom actions; MCP |
| **BoltAI** | Commercial BYOK AI workspace | Yes | **Yes**, 300+ models incl. Ollama | Native Mac; broad AI client; writing is one use case |
| **ProWritingAid** | Closed subscription | Yes | No | Deep reports; fiction-centric |
| **LanguageTool Premium** | Freemium/cloud | Yes | Partial (self-host) | Rules + paraphrasing + style guide |
| **Microsoft Editor / Google Gemini** | Closed | Limited/web | No | Ecosystem lock-in |
| **Raycast AI / ChatGPT / Claude desktop** | Closed | Yes | No | General AI, not proactive writing |

### 7.3 The empty intersection

```text
                     LOCAL / PRIVATE
                           ▲
                           │
        Harper Desktop     │      (empty)
        LanguageTool       │   Harper ergonomics
        Vale               │   + Grammarly judgment
                           │   + your key
                           │
  ─────────────────────────┼─────────────────────────►
     RULE-BASED            │            LLM-POWERED
                           │
        Apple Writing      │      Grammarly / Superhuman
        Tools              │      Kerlig / BoltAI
        Microsoft Editor   │      ProWritingAid
                           │
                     CLOUD / SUBSCRIPTION
```

- Top-left: private but rule-based (Harper, LanguageTool, Vale).
- Bottom-right: LLM-powered but cloud/subscription (Grammarly, Kerlig, BoltAI).
- Top-right: **private, LLM-powered, BYOK, cross-app, open source** — unoccupied.
- Bottom-left: free OS/ecosystem tools (Apple Writing Tools, Microsoft Editor).

Our position: **top-right**, built on Harper's local layer, differentiated by LLM judgment and provider choice.

### 7.4 A second differentiator: voice learning

Grammarly's personalization is cloud-side and packaging-oriented: brand tones, style guides, organizational dictionaries, and usage analytics learned across 50,000 organizations. It is useful, but it is not a transparent model of *your* voice that you can inspect, edit, export, or delete.

The competitors leave a specific gap:

- **Harper** has no LLM, so it cannot do voice-aware rewriting or drafting.
- **Kerlig/BoltAI** are BYOK but generic: they send your prompt to a model; they do not maintain a persistent, per-context model of your writing.
- **Grammarly** learns at the organization level and keeps the profile in its cloud.
- **Apple Writing Tools** offers tone controls, not a learned personal voice.

A **local voice profile** — built from uploaded samples and from the edits you accept, stored as an explainable style card plus a few representative excerpts, and injected into any provider's prompt — is a second defensible differentiator. It is provider-agnostic, private, editable, and it makes "your model" feel like *your* writing rather than a generic assistant.

It is also not fine-tuning: no corpus upload, no model lock-in, no retraining cost, no catastrophic forgetting. Full design: [`VOICE-LEARNING.md`](VOICE-LEARNING.md).

---

## 8. Build strategy options

This is the most consequential decision in the plan.

### Option A — Fork/vendor Harper Desktop (Tauri + Rust + Svelte) and add the LLM layer

**What we reuse (Apache-2.0):**
- `harper-core` local grammar engine (English, <10ms).
- `harper-desktop` macOS Accessibility broker: focused element, text extraction, range bounds, app catalog, window stability, secure-field handling.
- Highlighter subprocess and overlay windows.
- Tray, settings/config, per-app enable, updater, autostart.
- Windows broker (future cross-platform path).

**What we build:**
- Provider/BYOK layer (OpenRouter default, OpenAI-compatible, Anthropic, Gemini, Ollama, Apple FM later).
- LLM analysis pipeline: chunking, prompts, structured edits, offset validation, dedupe, cache.
- Voice learning: sample ingestion, local style card, feedback-weighted profile, per-context voices, copy guard.
- Grammarly-style suggestion card, floating widget, assistant panel.
- Onboarding, provider/model/cost UI, network log, privacy controls.
- Browser extension bridge and Chrome/Edge extension.
- Style guide, snippets, provenance, history, docs.
- New visual identity and product name.

**Pros:** fastest to a working overlay; proven hardest part; local+LLM hybrid; Windows path; Apache-2.0 compatible.
**Cons:** Rust + TypeScript + Objective-C/Swift; Tauri webview UI; inherit beta bugs; derivative work/attribution; must rename all identifiers and remove Harper branding; large monorepo.
**Estimate to MVP:** 4–6 weeks. **To v1.0:** 14–19 weeks (voice learning adds ~11–14 days).

### Option B — Native Swift app + `harper-core` via FFI

**What we reuse:** `harper-core` as a Rust static library (UniFFI/C FFI), plus Harper Desktop's AX code as a reference (ported to Swift).
**What we build:** everything else, including the AX overlay and highlighter in Swift.
**Pros:** truly native UX/performance; full control; Swift-only app layer; easier AppKit integration.
**Cons:** must implement/port the overlay, app catalog, window stability, highlighter; Rust FFI build; no Windows path; 4–8 weeks more.
**Estimate to MVP:** 7–10 weeks. **To v1.0:** 16–22 weeks.

### Option C — Clean-room native Swift + own rules

**Pros:** most original; no derivative-work questions; simplest licensing.
**Cons:** weakest local grammar; longest path; rebuilds solved problems; not recommended.
**Estimate to v1.0:** 20–28 weeks.

### Recommendation

**Option A for v1.** The product's differentiator is the LLM/BYOK/UX layer, not the Accessibility plumbing. Harper has already solved the plumbing and licensed it permissively. Forking lets us ship a credible product in months rather than quarters, while our original contribution — provider abstraction, judgment prompts, local voice learning, privacy, cost transparency, extension — remains substantial and defensible.

**Conditions:**
- Pin to Harper v2.13.0 (or a later audited tag), keep an upstream remote, and document the sync strategy.
- Retain Apache-2.0 notices and add a clear "built on Harper" attribution in README/About.
- Remove all Harper trademarks, icons, bundle IDs, config paths, and website references; rename the product.
- Do not imply endorsement by Automattic.
- If the M0 spike shows the Tauri shell cannot deliver the required UX/stability, fall back to Option B before building the LLM layer deeply.

---

## 9. Feasibility assessment

Scores: 5 = straightforward; 1 = research project.

| Capability | Feasibility | Why | Biggest risk |
| --- | --- | --- | --- |
| Menu-bar app + login item + updater | 5 | Tauri/Harper shell already does it | Rebranding/config migration |
| Read focused text via Accessibility | 5 | Harper's broker handles native + Chromium | Canvas/Electron edge cases |
| Inline underlines over other apps | 4 | Harper highlighter already aligns to AX geometry | Per-app quirks, Spaces, zoom |
| Replace/accept edits | 4 | AX writes + pasteboard fallback | Apps that reject AX writes; undo |
| Floating widget anchored per app | 4 | Native overlay windows | Focus stealing, multi-monitor |
| Browser extension | 4 | MV3 + local bridge | Google Docs canvas, Safari distribution |
| LLM suggestion quality | 4 | Strong models + schema + validation | Hallucinated spans, over-editing |
| Tone detection | 4 | Classification + cache | Cost if run on every keystroke |
| Long documents | 3 | Chunking + offset remap + dedupe | Context limits, cost, inconsistency |
| Voice learning | 4 | Local feature extraction + style card + few-shot | Sample quality, copy guard, creepiness |
| Plagiarism | 1 | No corpus | Cannot meet expectations; reframe |
| AI detection | 1 | No labeled data; contested | Reputational harm |
| Multi-provider | 4 | Most are OpenAI-compatible | API drift, structured-output differences |
| On-device inference | 4 | Apple FM (4K context) or Ollama | 4K limit; availability by region/device |
| Notarized distribution + updates | 4 | Tauri updater or Sparkle | Signing setup, Gatekeeper |
| Cross-app QA | 2 | Every app is a snowflake | Time sink; publish a matrix |

---

## 10. Risks

### Product
- Users expect Grammarly accuracy/coverage → publish a matrix, set expectations, make feedback easy.
- Latency annoys → local rules instantly, LLM debounced/on demand, cancel, cache.
- API costs surprise → cost meter, budget caps, cheap default, cache.
- Permission friction → explain value, deep-link, poll, playground.
- False positives → conservative prompts, confidence thresholds, accept/dismiss learning, category toggles.
- Privacy backlash → local mode, per-app rules, network log, no telemetry.
- Apple ships better free tools → differentiate on model choice, customization, proactivity, cross-provider, open source.
- Scope creep → phase gates, non-goals.
- Voice profile feels creepy/overfit → off by default, transparent style card, strength slider, freeze/reset, cap single-sample influence.
- Users expect true model training → explain the style-card approach; offer preview and control; local LoRA later.
- Impersonation/unauthorized samples → ownership attestation, brand labeling, acceptable-use policy.

### Technical
- AX inconsistency → adapter layer, extension, OCR fallback, matrix.
- Overlay misalignment → reuse Harper; test matrix; widget-first fallback.
- Text injection corruption → re-read before injecting; pasteboard primary, AX fast path; read-back verify; undo.
- TCC resets in dev → stable dev cert + fixed bundle ID.
- macOS changes → avoid private APIs; test betas.
- Provider drift → adapter fixtures, capability flags, graceful degradation.
- Structured output unsupported → `require_parameters`, JSON repair, model fallback.
- Key leakage → Keychain, no logs, redaction.
- Prompt injection from document text → text-as-data, delimiters, no tools in analysis.
- Local bridge exposure → 127.0.0.1, pairing token, Origin allowlist.
- Fork maintenance → pin upstream, document sync, upstream contributions.
- Voice copies sample text → 8-gram overlap guard; reject/regenerate; excerpt-only prompts.
- Voice samples leak → AES-256-GCM at rest; Keychain-wrapped key; style-card-only mode; delete cascades.
- Voice feedback loop converges on AI text → learn from user-authored text/edits; down-weight unedited AI acceptances.

### Legal/brand
- Grammarly/Superhuman trademark → distinct name/logo; nominative comparison only; no copied assets.
- Harper trademark/attribution → retain Apache-2.0 notices; clear "built on Harper"; no endorsement claim.
- Misleading plagiarism/AI claims → reframe or omit.
- Provider terms (humanizer/academic cheating) → acceptable-use policy.
- License compatibility → Apache-2.0 in, Apache-2.0 out; audit LGPL/MPL deps if added.
- Voice samples may contain third-party text → require ownership/permission attestation; allow deletion; do not train on them.

---

## 11. Recommendations

1. **Fork/vendor Harper Desktop (Apache-2.0) for the macOS shell and local rules; build the LLM/BYOK layer as our original contribution.**
2. **Position against Superhuman, not 2023 Grammarly.** "Private, key-owned, high-quality rewriting that works where you type."
3. **Lead with the architectural wedge:** Grammarly cannot run offline or use your model without cannibalizing its cloud business.
4. **Ship judgment, not just correctness.** Free local correctness is table stakes (Harper, Apple); charge nothing and win on clarity, tone, rewrites, compose, translate, humanize.
5. **Use a hybrid engine:** Harper local rules every keystroke; LLM on demand/debounced; never block typing on the network.
6. **Browser extension is mandatory** for Gmail/Notion/Slack/Linear; Chrome/Edge first, Safari later. Google Docs is out of scope in v1 (canvas).
7. **Skip plagiarism and AI detection as products.** Offer local corpus overlap, web similarity (clearly labeled), AI-phrase linting, and provenance.
8. **Style guide = YAML + RAG**, not ML. **Brand voice = few-shot samples**, not training.
9. **Natural-language preferences** instead of 40 toggles.
10. **Transparency as a feature:** network log, cost meter, preview request, local-only mode.
11. **Publish a tested app matrix, not a "1M+ apps" claim.**
12. **Plan 4–6 weeks to MVP, 14–19 weeks to v1.0** on Option A; add 6–10 weeks for Option B.
13. **Complete formal trademark clearance for Elocutio** (US/EU/UK, classes 9 and 42) before launch. Retain Apache-2.0 and Harper attribution.
14. **Make voice learning a first-class, local feature.** Build an explainable style card from uploads and feedback, keep samples encrypted on-device, never fine-tune a cloud model, and show exactly what is sent. It is the second differentiator after model choice.

---

## 12. Sources

**Grammarly / Superhuman**
- Features: <https://www.grammarly.com/features>
- Plans: <https://www.grammarly.com/plans>
- Desktop: <https://www.grammarly.com/desktop>
- Mac: <https://www.grammarly.com/desktop/mac>
- Press: <https://www.grammarly.com/press>
- Superhuman Go: <https://www.superhuman.com/go>
- Superhuman Platform: <https://superhuman.com/superhuman-platform>
- GPTZero acquisition: <https://blog.superhuman.com/superhuman-to-acquire-gptzero/>
- Desktop user guide: <https://support.grammarly.com/hc/en-us/articles/4412816078349-Grammarly-for-Windows-and-Grammarly-for-Mac-user-guide>
- Go Mac guide: <https://support.grammarly.com/hc/en-us/articles/45113077035789>
- System requirements: <https://support.grammarly.com/hc/en-us/articles/44209059689869>
- Jamf deployment: <https://support.grammarly.com/hc/en-us/articles/8341875702413>
- Known issues: <https://support.grammarly.com/hc/en-us/articles/45183836343437>
- Privacy/security FAQ: <https://support.grammarly.com/hc/en-us/articles/20916119474829>
- Google Docs support: <https://support.grammarly.com/hc/en-us/articles/115000090991>
- Language support: <https://support.grammarly.com/hc/en-us/articles/115000090971>
- Wikipedia: <https://en.wikipedia.org/wiki/Grammarly>

**Open source / competitors**
- Harper: <https://github.com/Automattic/harper>
- Harper Desktop: <https://writewithharper.com/desktop>
- Harper desktop source: <https://github.com/Automattic/harper/tree/master/harper-desktop>
- LanguageTool: <https://github.com/languagetool-org/languagetool>
- Vale: <https://github.com/vale-cli/vale>
- textlint: <https://github.com/textlint/textlint>
- LTeX+: <https://github.com/ltex-plus/ltex-ls-plus>
- proselint: <https://github.com/amperser/proselint>
- Kerlig: <https://www.kerlig.com/>
- BoltAI: <https://boltai.com/>
- Apple Writing Tools: <https://support.apple.com/en-us/121115>

**Platform / providers**
- OpenRouter API: <https://openrouter.ai/docs/api_reference/overview>
- OpenRouter structured outputs: <https://openrouter.ai/docs/guides/features/structured-outputs>
- OpenRouter routing: <https://openrouter.ai/docs/guides/routing/provider-selection>
- OpenRouter limits: <https://openrouter.ai/docs/api_reference/limits>
- OpenRouter models: <https://openrouter.ai/api/v1/models>
- Apple Foundation Models: <https://developer.apple.com/documentation/foundationmodels>
- Apple Foundation Models context window: <https://developer.apple.com/documentation/foundationmodels/managing-the-context-window>
- Apple AXUIElement: <https://developer.apple.com/documentation/applicationservices/axuielement>
- Apple SMAppService: <https://developer.apple.com/documentation/servicemanagement/smappservice>
- Tauri: <https://tauri.app/>

---

## 13. Open questions

1. Exact rebrand date (press release date not rendered) — affects launch narrative only.
2. Superhuman Go/Mail/Docs list pricing (JS-gated/sales-led).
3. Grammarly Enterprise and Education per-seat pricing.
4. Independent verification of "99% AI detection" / "#1 RAID" / "1M+ apps."
5. Harper Desktop's AX behavior in secure fields, canvas editors, and Electron — read its source/tests before designing ours.
6. Which app matrix the target users actually need (Mail, Word, Docs, Slack, LinkedIn, Notion?).
7. Whether the user wants a derivative fork (Option A) or a clean native build (Option B) — **decided: Option A** on 2026-10-19.
8. How much writing is needed for a trustworthy voice profile, and whether users prefer uploading samples or letting the app learn from edits.
9. Whether a local style card + exemplars is sufficient for nuances (humor, empathy, disagreement), or whether local LoRA fine-tuning is worth the complexity for Ollama/MLX users.
