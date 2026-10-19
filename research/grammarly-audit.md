# Grammarly / Superhuman — Product & Feature Audit

**Prepared:** 19 October 2026
**Purpose:** Competitive and prior-art audit for an open-source, bring-your-own-API-key (BYOK) writing assistant for macOS.
**Scope:** grammarly.com, superhuman.com, Grammarly/Superhuman Help Center (Zendesk API), GitHub API, vendor sites for OSS comparables.

---

## 0. How this was researched (read this first)

`web_search` was **unavailable** in this session (its endpoint returned `HTTP 404 /api/web_search/messages not found`). All findings below therefore come from **direct fetches** of primary sources: marketing pages, the Grammarly Help Center's public Zendesk API (528 articles enumerated), the GitHub REST API, and vendor documentation. Search-engine corroboration (news coverage, independent benchmarks) is **missing** and is the main gap in this audit.

Consequences for confidence:

| Area | Confidence | Note |
|---|---|---|
| Feature inventory, surfaces, help-center behavior | **High** | Primary vendor docs, updated Sept–Oct 2026 |
| Consumer pricing (Free / Pro) | **High** | Rendered on live pages 19 Oct 2026 |
| Enterprise & Education pricing | **Low** | Contact-sales only; no public numbers |
| Superhuman plan pricing (Go/Mail/Docs) | **Low** | Pricing page is JS-rendered; no static numbers retrievable |
| Rebrand timeline | **Medium** | Press releases confirmed; exact rebrand date not stated on page |
| OSS comparables (licenses, activity) | **High** | GitHub API, 19 Oct 2026 |
| Independent AI-detector accuracy claims | **Low** | Vendor claims only; no independent verification possible here |

Prices are US list prices in USD as rendered on 19 Oct 2026 and vary by region, currency, promotions, and billing interval.

---

## 1. Corporate status: the branding changed, and this reframes everything

The premise in the brief — "rebranded / merged with Superhuman in 2025" — is **half right and worth correcting precisely**, because it affects naming, packaging, and who the competitor actually is.

**Sequence of events:**

1. **2024** — Grammarly acquires **Coda**; Coda CEO Shishir Mehrotra becomes Grammarly CEO. ([Wikipedia](https://en.wikipedia.org/wiki/Grammarly))
2. **2025** — Grammarly agrees to acquire **Superhuman** (the email client). Press release title: *"Grammarly to Acquire Superhuman to Accelerate Its AI Productivity Platform."* ([press index](https://www.grammarly.com/press))
3. **2026** — The **parent company rebrands as Superhuman** and launches the **Superhuman Suite** plus **Superhuman Go**. Press release: *"Grammarly Rebrands Company as Superhuman, Introduces Superhuman Suite and Superhuman Go."* ([press index](https://www.grammarly.com/press))
4. **23 June 2026** — Superhuman agrees to acquire **GPTZero** (AI detection, hallucination detection, plagiarism, AI Vision). ([announcement](https://blog.superhuman.com/superhuman-to-acquire-gptzero/))

So: the acquisition closed in **2025**, but the **rebrand is a 2026 event**. Wikipedia's phrasing — *"After buying Superhuman in 2025, Grammarly rebranded its parent company as Superhuman"* — matches this. ([Wikipedia](https://en.wikipedia.org/wiki/Grammarly))

**Current product architecture** ([Superhuman Platform](https://superhuman.com/superhuman-platform)):

| Product | What it is | Status of the Grammarly name |
|---|---|---|
| **Superhuman Go** | Proactive AI assistant + agent platform, "works in 1M+ apps and websites" | Grammarly is now an **agent inside Go** |
| **Grammarly** | "The writing assistant trusted by millions" | **Name retained** as the writing product and agent |
| **Superhuman Docs** | AI-native docs/collaboration surface (Coda lineage) | Replaces/absorbs the Grammarly Editor + "docs" |
| **Superhuman Mail** | Email client | Formerly Superhuman Mail |

**Practical naming reality as of Oct 2026:**

- **grammarly.com still sells "Grammarly Free" and "Grammarly Pro"** at consumer prices; the Grammarly brand is alive at the individual tier.
- **Enterprise is now sold as Superhuman Go.** The plans page literally reads: *"Enterprise — Drive results across your organization with Grammarly, available through Superhuman Go."* ([plans](https://www.grammarly.com/plans))
- Help Center: *"Grammarly is now part of Superhuman, a suite of AI tools that includes Superhuman Go, Grammarly, Coda, and Superhuman Mail."* ([Learn about Grammarly Pro](https://support.grammarly.com/hc/en-us/articles/25043801131149-Learn-about-Grammarly-Pro))
- Installing Grammarly on Windows now shows a notification *"You're using Superhuman Go…"* ([article](https://support.grammarly.com/hc/en-us/articles/47212483110029))
- The **desktop client is being renamed**: "Grammarly for Windows and Mac" and "Superhuman Go for Windows and Mac" are **two separate apps that cannot run at the same time**. ([coexistence article](https://support.grammarly.com/hc/en-us/articles/45976306096781))

> **Implication for us:** the competitor is no longer "Grammarly the grammar checker." It is **Superhuman, an agent platform whose wedge is inline suggestions in 1M+ apps**. The grammar checker is now one agent among ~100. Our differentiation should not be "better grammar underlines" (a commoditized, 17-year-old feature) but **private, key-owned, high-quality rewriting** — the thing Grammarly monetizes and the thing it cannot do without your text on its servers.

---

## 2. Feature inventory

Legend for **Cloud**: Grammarly has **no documented offline mode**. Client apps are thin: they capture text via OS/browser accessibility, send it to Grammarly's servers, and render returned annotations. So "Cloud: Yes" below means *"requires Grammarly's cloud"* — not merely "uses an LLM." This is the single most important architectural fact for a local BYOK competitor: **every Grammarly feature, including spelling, is a network round-trip.**

Tier abbreviations: **F** = Free, **P** = Pro (and legacy Plus/Premium), **E** = Enterprise/Business, **Ed** = Education.

### 2.1 Correctness

| Feature | Description | Surfaces | Tier | Cloud |
|---|---|---|---|---|
| Grammar correction | Subject-verb agreement, tense, articles, prepositions, fragments, run-ons | All | F | Yes |
| Spelling | Misspellings incl. typos, with "Add to dictionary" | All | F | Yes |
| Punctuation | Commas, semicolons, colons, em/en dashes, apostrophes, quotation marks, Oxford comma | All | F | Yes |
| Capitalization | Sentence starts, proper nouns, titles, days/months | All | F | Yes |
| Commonly confused words | their/there/they're, affect/effect, then/than, lose/loose | All | F | Yes |
| Formatting/hygiene | Stray spaces, doubled words, mismatched parentheses/quotes | All | F | Yes |
| Dialect consistency | US/UK/CA/AU/IN English spelling & punctuation | All | F | Yes |
| Multilingual correctness | Suggestions in **24 languages** (see §2.10) | All | F | Yes |

Free-tier behaviour: correctness issues are **red underlines**, shown inline without opening a sidebar. ([Google Docs article](https://support.grammarly.com/hc/en-us/articles/115000090991))

### 2.2 Clarity

| Feature | Description | Surfaces | Tier | Cloud |
|---|---|---|---|---|
| Conciseness | Trims redundant words/phrases | All | **P** | Yes |
| Readability | Flags dense/awkward sentence structure; readability scoring | All | **P** | Yes |
| Passive voice | Detects passive constructions, suggests active | All | **P** | Yes |
| Wordiness | "Due to the fact that" → "because" style compression | All | **P** | Yes |
| Hedging / vague language | Flags weasel words and unnecessary qualifiers | All | **P** | Yes |
| Sentence structure | Run-ons, overlong sentences, tangled clauses | All | **P** | Yes |
| Full-sentence rewrite | One-click rewrite of a whole sentence | All | **P** | Yes |
| Paragraph-level rewrites | Rewrite whole paragraphs; base for tone/translate | Win/Mac, Go, extension | **P** | Yes |
| Translation | 19 languages, 4,000 char limit, inside paragraph rewrites | Win/Mac, Go, Chrome ext | **P** | Yes |

Clarity issues are **blue underlines** and are explicitly gated: *"If you're on a paid plan, you will also likely see blue underlines for more complex clarity and tone suggestions."* ([Docs article](https://support.grammarly.com/hc/en-us/articles/38552281546765))

### 2.3 Engagement

| Feature | Description | Surfaces | Tier | Cloud |
|---|---|---|---|---|
| Vocabulary enhancement | Word-choice upgrades; double-click a word for synonyms | All | **P** | Yes |
| Word choice / precision | "very important" → "critical"; connotation fixes | All | **P** | Yes |
| Sentence variety | Flags monotonous sentence openings/length | All | **P** | Yes |
| Repetitive words | Repeated-word and repeated-phrase detection | All | **P** | Yes |
| Compelling language | Strengthens weak verbs, intensifiers | All | **P** | Yes |

Marketing framing confirms this axis: *"Find the perfect words… recommends vocabulary improvements."* ([Mac page](https://www.grammarly.com/desktop/mac))

### 2.4 Delivery (tone, formality, inclusivity)

| Feature | Description | Surfaces | Tier | Cloud |
|---|---|---|---|---|
| Tone detection | Emoji + label for how writing sounds; needs 150–2,000 chars | All, incl. floating widget | **F** | Yes |
| Tone adjustment | Rewrite toward a target tone in one click | All | **P** | Yes |
| Formality control | Shift formal ↔ casual | All | **P** | Yes |
| Confidence/assertiveness | Soften or harden claims | All | **P** | Yes |
| Inclusive language | Flags gendered, ableist, otherwise exclusionary phrasing | All | **P** | Yes |
| Brand tones | Org tone profile(s); Pro allows 1, Enterprise unlimited | All | **P**/**E** | Yes |
| Reader Reactions | Predicts how a chosen audience will read the text; surfaces likely takeaways + questions | Docs, Go | **P** | Yes |
| Strategic suggestions | Suggestions on *what* to include to land with an audience | Business/Enterprise | **E** | Yes |

Sources: [plans](https://www.grammarly.com/plans), [tone suggestions](https://support.grammarly.com/hc/en-us/articles/10674801783309), [brand tones](https://support.grammarly.com/hc/en-us/articles/4403544890253), [business](https://www.grammarly.com/business).

### 2.5 Originality / authenticity

| Feature | Description | Surfaces | Tier | Cloud |
|---|---|---|---|---|
| **Plagiarism checker** | Checks text against "billions of web pages" + academic/professional sources; originality score; citation suggestions | docs/web; **desktop: Word only** | **P** | Yes |
| **AI detector** | % of text resembling AI; claims "99% accuracy", "#1 on RAID benchmark" | docs/web; **desktop: Word only** | **P** | Yes |
| **Humanizer** | Rewrites AI-sounding text to read natural while keeping meaning | Go, docs, homepage | **P** | Yes |
| **AI Rewriter** | Flags phrases AI overuses ("Almost always AI" / "Usually AI" / "Sometimes AI") and offers less-AI alternatives | docs | **P** | Yes |
| **Authorship** | Records the writing *process*: typed vs pasted vs AI-generated; shareable report | Google Docs, Word, docs | F (basic) / **P** (detailed) | Yes |
| **Citation generator** | APA/MLA/Chicago bibliography generation + in-text citations | Web tool, docs, extension | F (basic) / **P** | Yes |
| Citation Finder | Finds sources that support/contradict your claims; pre-formatted citations | docs | **P** | Yes |
| AI Grader | Instructor/rubric-based "pregrade" with improvement tips | docs (Education) | **Ed** | Yes |
| Knowledge Checker | Verifies claims against company data sources; fixes inline | Go | **E** | Yes |

Notable: **plagiarism and AI detection are crippled on macOS** — *"the plagiarism checker and AI detector features are available in Superhuman Go for Mac only in Microsoft Word."* ([Go Mac guide](https://support.grammarly.com/hc/en-us/articles/45113077035789)) On the web they work anywhere.

### 2.6 Generative AI

| Feature | Description | Surfaces | Tier | Cloud |
|---|---|---|---|---|
| Prompt-based generation | Compose drafts, outlines, ideas from a prompt | All | **F**: 100/mo · **P**: 2,000/mo · **E**: unlimited | Yes |
| Compose / draft | Blank-page drafting in place | All | F (limited) | Yes |
| Reply | Draft replies in context (email, Slack) | Go, Mail | **P** | Yes |
| Summarize | Summarize selection/thread/window | All | **P** | Yes |
| Rewrite / "Improve it" | Rewrite selection with intent (shorten, formalize, etc.) | All | **P** | Yes |
| Tone adjustment via prompt | "Make this warmer" natural-language instruction | All | **P** | Yes |
| AI Chat (side panel) | Chat grounded in the active window's text | Win/Mac, Go | F/P | Yes |
| Suggested prompts | Context-aware prompt chips | All | F | Yes |
| Prompt history | View past generations | Web | P | Yes |
| Reading assistance: Ask Go / Translate / Explain | Select text on any page → ask, translate, plain-language explain | Go extension | F/P | Yes |

Prompt limits are inconsistent across surfaces and worth noting as a packaging lever: **100/mo (Free), 2,000/mo (Pro), unlimited (Enterprise)** on the plans page, but the **desktop Go app allows 1,000 prompts/day** regardless. ([plans](https://www.grammarly.com/plans) · [Go Mac guide](https://support.grammarly.com/hc/en-us/articles/45113077035789))

### 2.7 Documents & knowledge surfaces

| Feature | Description | Surfaces | Tier | Cloud |
|---|---|---|---|---|
| **Docs** (AI writing surface) | Google-Docs-like editor with an agent panel; slash commands, tables, outline, export to PDF/docx | app.grammarly.com, docs.superhuman.com | F entry / **P** agents | Yes |
| Grammarly Editor (legacy) | Original web editor; existing docs now labelled "Classic" | app.grammarly.com | F | Yes |
| Agent panel | Right-side rail hosting ~11 agents per document | Docs | F/P | Yes |
| **Suggestion cards** | The core interaction: hover an underline → card with change, explanation, Accept/Dismiss, "Learn more" | All | F | Yes |
| **Knowledge Share** | Tooltips for internal acronyms/project names, with definitions, owners, doc links | Chrome: Google Docs, Gmail, Confluence, Jira · **Mac: Slack** · Win: Word/Outlook | **P** | Yes |
| **Snippets** | Text expansion library; `\` opens picker; CSV import; per-group collections | Win/Mac, extension | **P** | Yes |
| Advanced snippets | Templates with dropdowns, inputs, expandable content, name/email, date/time | Win/Mac, extension | **E**/**Ed** | Yes |
| Personal dictionary | Stop flagging your own words | All | F | Yes |
| Organizational dictionary | Team-wide accepted terms; CSV upload | Web admin | **P** | Yes |
| Style guide | Upload company style guide; suggestions enforce it | All | **P** (1) / **E** (unlimited) | Yes |
| Analytics | Team/individual usage, adoption, ROI reporting | Web admin | **P**/**E** | Yes |
| Feature access / management hub | Admin toggles per feature, group, role | Web admin | **E** | n/a |

Sources: [Docs](https://support.grammarly.com/hc/en-us/articles/38552281546765), [Knowledge Share](https://support.grammarly.com/hc/en-us/articles/16664924710797), [snippets](https://support.grammarly.com/hc/en-us/articles/4403077145485), [personal dictionary](https://support.grammarly.com/hc/en-us/articles/115000091491), [org dictionary](https://support.grammarly.com/hc/en-us/articles/14441067751309).

### 2.8 The 2025–26 "agents" layer (Superhuman Go)

Go is a **platform**, not a feature. Its architecture ([superhuman.com/go](https://www.superhuman.com/go)):

| Concept | Detail |
|---|---|
| **Proactive suggestions** | "Go doesn't wait for a prompt" — annotates as you type, everywhere |
| **Agent team** | You curate specialists; Go decides when/where they appear |
| **Built-in agents** | Grammarly, Email Assistant, Knowledge Checker, Daily Brief, Humanizer, AI Detector |
| **Custom agents** | No-code builder; trigger **while writing**, **on a schedule**, or **on an event** (meeting ends, email arrives) |
| **Agent Store** | "100+ agents, ready to go" from Superhuman and partners (Box, Gamma, Wayground) |
| **Connectors** | Pre-built + **custom MCP** connectors to ground answers in your data |
| **Surfaces** | Inline underline · side panel chat · **on-cursor actions** (highlight text → action list) · **Slack** (@-mention Go) · Go app · docs |
| **Agents SDK** | For developers to build org-specific agents |
| **Attribution** | Agent-specific attribution shipped with the Authorship update |

**Grammarly-as-agent is now customizable in natural language**: *"Tell Grammarly what edits you find useful, and it'll only show you the underlines that matter."* Users can also type free-form preferences ("Write AM and PM in uppercase", "Enforce American English dialect") which are **saved as agent settings** — not a fixed rule list. ([customize the Grammarly agent](https://support.grammarly.com/hc/en-us/articles/48202938578317))

This is a meaningful competitive shift: **Grammarly's suggestion engine is now steerable by prompt**, which is exactly the affordance a BYOK app would want to offer. It also means the "suggestion categories" model (correctness/clarity/engagement/delivery) is being replaced by a learned, per-user preference model.

### 2.9 Client surfaces

| Surface | Platforms | Notes | Tier |
|---|---|---|---|
| Browser extension | Chrome, Edge, Safari, Firefox | Google Docs, Gmail, LinkedIn, WordPress, 1M+ sites; per-site toggles | F |
| Desktop app | **Windows, macOS** | System-wide overlay via accessibility; floating widget + edge tab | F |
| **Superhuman Go** app | Win, macOS, Chrome, Edge, iOS, Android, web (go.superhuman.com) | Successor client; **mutually exclusive** with Grammarly desktop | F |
| Web editor / Docs | Any browser | app.grammarly.com | F |
| iOS / iPadOS | Native app + **Grammarly Keyboard** + Safari extension | Keyboard works in any app | F |
| Android | Native app + Grammarly Keyboard | Keyboard works in any app | F |
| MS Word | Via desktop app (Win/Mac) | **Word-on-Mac add-in discontinued**; Office add-in retiring | F |
| MS Outlook | Via desktop app (Win/Mac) | Native integration | F |
| Google Docs | Via browser extension **only** | Desktop app does **not** support it | F |
| Slack | Via desktop app | Mac: Knowledge Share works here | F |
| Linux | **No native app** | Web/extension only | F |

Discontinuations that matter for surface parity: **Grammarly for Microsoft Word on Mac was discontinued** ([article](https://support.grammarly.com/hc/en-us/articles/39667635821325)); **Grammarly for Microsoft Office (add-in) is being retired** and no longer gets updates ([article](https://support.grammarly.com/hc/en-us/articles/39983075497485)); **Grammarly's expert human writing service ended 31 Jan 2025** ([article](https://support.grammarly.com/hc/en-us/articles/34083794138381)); **Expert Review** (attributing edits to named subject-matter experts) was **discontinued in March 2026** after criticism ([Wikipedia](https://en.wikipedia.org/wiki/Grammarly)).

### 2.10 Language coverage (a 2026 expansion worth noting)

Beyond English, suggestions are supported in **24 languages**: Spanish, French, Italian, German, Portuguese, Turkish, Polish, Dutch, Czech, Ukrainian, Vietnamese, Hungarian, Swedish, Romanian, Indonesian, Slovak, Danish, Finnish, Norwegian, Tagalog, Korean, Hindi, Japanese, Chinese. Paragraph-level rewrites: Spanish, French, Italian, German, Portuguese, Ukrainian. Translation: 19 languages. **All plans, including Free.** ([language support](https://support.grammarly.com/hc/en-us/articles/115000090971) · [translation](https://support.grammarly.com/hc/en-us/articles/27518589299725))

Five English dialects: US, UK, CA, AU, IN.

> For a BYOK app this is **good news**: multilingual quality falls out of the LLM for free, so a small team can match Grammarly's language breadth on day one — something rule-based incumbents (LanguageTool aside) cannot.

---

## 3. macOS deep dive

### 3.1 What it installs

From the MDM/Jamf deployment script ([deploy Grammarly for Mac](https://support.grammarly.com/hc/en-us/articles/8341875702413)):

- **Format:** `.dmg` from `https://download-mac.grammarly.com/Grammarly.dmg`, volume name `Grammarly`.
- **Bundled object:** `Grammarly Installer.app` copied to `/Applications/Grammarly Desktop.app` (or `~/Applications` for non-admins).
- **Bundle identifier:** `com.grammarly.ProjectLlama` (inferred from `defaults write com.grammarly.ProjectLlama deferOnboarding -bool YES`).
- **Quarantine stripped** by the script (`xattr -r -d com.apple.quarantine`), i.e. they expect MDM-signed rollout.
- **First-run:** launched with `--args launchSourceInstaller`; onboarding can be deferred via the `deferOnboarding` default.
- **Login item:** it must run at login to intercept keystrokes in other apps; the menu-bar icon is the persistent UI. (The script launches it immediately after install; there is no daemon/launchd plist in the script, so persistence is the standard `SMAppService` login-item mechanism rather than a root LaunchDaemon.)
- **Accessibility permission is mandatory and pre-grantable by MDM:** step 2 of the deployment guide is *"Create the Configuration profile to allow Grammarly for Mac to access the Accessibility."* This is the crux of the architecture — see §3.3.
- **Also requires a browser engine present:** system requirements list *"Compatible version of Google Chrome (89 or newer) or Safari (12.1 or newer)"* — the client embeds/hosts web content. ([system requirements](https://support.grammarly.com/hc/en-us/articles/44209059689869))
- **OS floor:** **macOS 11 (Big Sur)+** for Go; **Apple Silicon or Intel Mac from 2018+**. (The consumer Mac page still says macOS 10.15+, which is stale relative to the requirements article — flagged.) ([system requirements](https://support.grammarly.com/hc/en-us/articles/44209059689869))

### 3.2 How it runs in the background

- **Menu-bar app.** Icon in the macOS menu bar is the control surface: Settings, "I write in" dialect, Block List, "Turn off Superhuman Go tab", "Turn off for 1 hour", "Turn off Superhuman Go". ([Go Mac guide](https://support.grammarly.com/hc/en-us/articles/45113077035789))
- **Always resident.** Grammarly/Go must be running for underlines to appear anywhere; launching from `/Applications` restores the menu-bar icon if it disappears.
- **Mutual exclusion:** Grammarly desktop and Go desktop **cannot both be active**; switching between them prompts, and the inactive one shuts down. ([coexistence](https://support.grammarly.com/hc/en-us/articles/45976306096781))
- **Per-app opt-out with a block list** persisted in Settings → Block List, plus "turn off for this app/site for 1 hour". ([choose where it works](https://support.grammarly.com/hc/en-us/articles/44208969169037))

### 3.3 How suggestions are surfaced inside other apps

Two distinct UI forms, both present simultaneously:

1. **Inline underlines** drawn *inside* the host application's text field — the app reads text and renders its own underline overlay on top of the foreign window's text coordinates.
2. **Right-edge vertical tab / side panel** ("Superhuman Go tab"/"Grammarly tab"), repositionable vertically, hideable while keeping underlines. Contains Review suggestions, the agent list (Grammarly, AI Chat, Humanizer, etc.), and a chat box that **uses the active window as context**.
3. **Floating widget** (legacy Grammarly client): a small draggable widget anchored to text fields with magnetic "anchor points"; shows a live count of suggestions and a **tone emoji**; position is **remembered per app/site**. ([legacy user guide](https://support.grammarly.com/hc/en-us/articles/4412816078349))
4. **Selection-based actions:** highlight text → a **blue sidebar appears on the left** with preset quick actions (shorten, formalize, translate…) plus **"Revise with AI"** for a custom prompt → **Accept** to insert.
5. **Suggestion cards** on hover: change preview, explanation, Accept / Dismiss / three-dot menu ("Hide suggestions like this", "Edit settings", report).

This is an **accessibility-API overlay**, not an input-method hack: the MDM requirement to pre-approve Accessibility confirms it, and it is the only way to draw inside Slack/Pages/Mail. (See §5 — Harper Desktop and Kerlig use the same approach.)

### 3.4 Keyboard shortcuts (macOS)

| Action | Shortcut |
|---|---|
| Open the Grammarly assistant | `Shift`+`Ctrl`+`Cmd`+`G` |
| Open AI Chat directly | `Ctrl`+`G` |
| Toggle (open/close) the sidebar | `Shift`+`Cmd`+`G` |
| Open snippets picker | `\` |
| Create snippet from within the app | `Ctrl`+`Shift`+`S` |
| Navigate UI | `Tab` / `Shift`+`Tab`; activate with `Enter`/`Space` |

Sources: [keyboard navigation](https://support.grammarly.com/hc/en-us/articles/24059143083277), [snippets](https://support.grammarly.com/hc/en-us/articles/4403077145485). Docs editor shortcuts are conventional: `⌘B/I/U`, `⌘Z`, `⇧⌘Z`, slash commands (`/h1`, `/table`, `/quote`).

Note the shortcut namespace is crowded — `Ctrl`+`G`, `Shift`+`Cmd`+`G`, and a bare `\` for expansion. A new entrant should pick a collision-free chord and make it user-configurable from day one.

### 3.5 Supported apps (and what "1 million+" actually means)

Grammarly's own list: **Microsoft Word, Outlook, PowerPoint, Teams, Pages, Notes, Slack, Discord, Apple Mail, Gmail, Google Docs, Sheets, Confluence, Jira, Salesforce, HubSpot, Zendesk, LinkedIn, WordPress, Medium, YouTube, X, Facebook, Instagram, Yahoo.** "1 million+ apps and programs" is a claim about *any* app with a standard text field, not a verified integration list.

Where each surface is required:

- **Native apps** (Mail, Pages, Slack, Discord, Word, Outlook, Teams, Notes) → desktop app.
- **Web apps** → browser extension *or* desktop app.
- **Google Docs** → **browser extension only**; desktop app is not supported there. ([Google Docs support](https://support.grammarly.com/hc/en-us/articles/115000090991))
- **Google Sheets** → desktop app; **Google Slides is not supported**. ([known issues](https://support.grammarly.com/hc/en-us/articles/45183836343437))

### 3.6 Known limitations (macOS-relevant, all vendor-documented)

| Limitation | Detail | Source |
|---|---|---|
| **Plagiarism + AI detection only in Word** | Not available in Mail, Slack, Docs, Pages on Mac | [Go Mac guide](https://support.grammarly.com/hc/en-us/articles/45113077035789) |
| **Google Docs needs the extension** | Desktop app can't hook Docs' canvas | [Google Docs](https://support.grammarly.com/hc/en-us/articles/115000090991) |
| **Gmail subject line not checked** | Vendor-documented gap; workaround = paste into body | [known issues](https://support.grammarly.com/hc/en-us/articles/45183836343437) |
| **Docs break with tables/footnotes** | Inline underlines fail; workaround = remove, edit, re-add | same |
| **Docs ≥30 pages slow down** | Performance cliff on long documents | same |
| **PowerPoint on Mac misbehaves** | "Grammarly for Mac doesn't function correctly on Microsoft PowerPoint" | [article](https://support.grammarly.com/hc/en-us/articles/41821160010637) |
| **Authorship capped at 20 pages** in Word | Tracked history becomes unreliable past the cap | [Authorship](https://support.grammarly.com/hc/en-us/articles/29548735595405) |
| **Only one client active** | Grammarly and Go cannot run together | [coexistence](https://support.grammarly.com/hc/en-us/articles/45976306096781) |
| **Electron apps** | Work (Slack/Discord are Electron), but underlines depend on the app's AX tree — apps with custom-drawn text (canvas/WebGL editors) are the usual failure class | *inference* |
| **Secure input fields / password managers** | When macOS **Secure Event Input** is active, the process is blocked from reading keystrokes/text by the OS. Apple's own password fields and most password managers engage this. Expect no suggestions in password, card, and some banking fields. | *inference from macOS Secure Event Input semantics; not explicitly documented by Grammarly* |
| **Cloud dependency** | No offline mode; all checking is server-side | *inference from architecture; no offline doc found* |
| **Auth fragility** | Multiple help articles on being logged out / unable to log in to the desktop app | [article](https://support.grammarly.com/hc/en-us/articles/6580956538765) |

**Honest read:** the "works in 1M+ apps" claim is true for *well-behaved native/standard text fields*, and progressively less true for: Google Docs (needs extension), long/complex docs, canvas-rendered editors, secure fields, and anything Apple changes in a macOS release. **Every one of these is a bug class we will inherit if we copy the AX-overlay approach** — budget for it explicitly.

---

## 4. Pricing & packaging

### 4.1 Consumer tiers (verified live, 19 Oct 2026)

| Tier | Price | Includes | Notes |
|---|---|---|---|
| **Free** | **$0** | "Write without mistakes"; **see** your tone; **100 AI prompts/month** | Correctness + tone *detection* + limited generation |
| **Pro** (annual) | **$12**/member/mo, billed annually | Everything in Free + rewrite full sentences, *adjust* tone, stay on-brand, **2,000 prompts/mo**, plagiarism + AI detection, unlimited personalized suggestions | 1–149 members; **billed as one payment for all seats** |
| **Pro** (monthly) | **$30**/member/mo | same | "Save up to 60%" vs monthly |
| **Premium** (App Store / Play) | App-store pricing | All core writing features, 1,000 prompts/mo, **no** team features | Legacy mobile IAP; not migrating to Pro |
| **Enterprise** | **Contact sales** | Everything in Pro + **via Superhuman Go**: proactive AI everywhere, **unlimited prompts**, unlimited members, dedicated support, **BYOK encryption**, custom roles, DLP, SSO/SAML, SCIM, audit-log API, confidential mode, cost-center visibility, group-level controls, application/domain controls | Sold as *Go*, not Grammarly |
| **Grammarly for Education** | **Contact sales** | Authorship, AI Grader, LMS integration; 3,000+ institutions | Per-institution seat deals |

Sources: [plans](https://www.grammarly.com/plans) · [homepage pricing block](https://www.grammarly.com/) · [Pro article](https://support.grammarly.com/hc/en-us/articles/25043801131149) · [edu](https://www.grammarly.com/edu).

### 4.2 Packaging logic worth stealing (and avoiding)

- **The paywall is drawn at Clarity/Engagement/Delivery, not at Correctness.** Free gets red underlines; blue underlines (clarity, tone, vocabulary) are paid. This is a deliberate and effective split: correctness is table stakes, judgement is the product.
- **Prompt quotas are the metering unit** (100 / 2,000 / unlimited). Cheap to implement, easy to reason about, and maps naturally onto *a user's own API spend* in a BYOK model — but in BYOK the quota is pointless, so we should meter on **nothing** and make that the pitch.
- **Seat model:** Pro = self-serve up to 149 seats, one payment, prorated adds. Enterprise = sales-led. Education = institution-led.
- **Team features are the upsell from Pro** (style guide ×1, brand tones ×1, Knowledge Share, snippets, analytics). Enterprise uncaps them.

### 4.3 Two "BYOK"s — do not confuse them

Grammarly Enterprise advertises **"BYOK encryption"** and **"Enterprise key management (BYOK)"**. That is **bring-your-own-*encryption*-key** (customer-managed KMS keys for data at rest) — **not** bring-your-own-API-key. Grammarly never lets you supply your own model or provider. ([privacy FAQ](https://support.grammarly.com/hc/en-us/articles/20916119474829): *"Enterprise clients can either provide their own encryption keys or use a Grammarly-managed key"*)

> **This is our clearest, most defensible wedge.** Grammarly's model is structurally the opposite of ours: your text must reach their AWS us-east infrastructure for *any* analysis, including spelling. Our claim — *the text never leaves the Mac, and the model is yours* — is not a feature Grammarly can ship without destroying its business model.

### 4.4 Superhuman plan pricing

Not retrievable: `superhuman.com/pricing` is a JS-rendered funnel (2.5 KB of HTML, no numbers). What *is* documented: **Go is free to start and included in every Superhuman plan**; **custom agents require Pro, Business, or Enterprise**. ([Go FAQ](https://www.superhuman.com/go)) Treat Superhuman tier pricing as **unknown / sales-led** in any comparison table.

---

## 5. Competitive & open-source landscape

### 5.1 Open-source / self-hosted

All license and activity data pulled from the GitHub API on **19 Oct 2026**.

| Project | License | Approach | macOS | BYOK / BYO-model | Activity | Lesson for us |
|---|---|---|---|---|---|---|
| **[Harper](https://github.com/Automattic/harper)** | **Apache-2.0** | Rule-based, Rust, fully local, English only | **Yes — Harper Desktop, system-wide, macOS 14+, beta** | No (no LLM at all) | Very active, **v2.13.0 (16 Oct 2026)**, ~16.7k★ | **The most important comparable. See §5.2.** |
| **[LanguageTool](https://github.com/languagetool-org/languagetool)** | **LGPL-2.1** | Rule/pattern-based + n-gram; 25+ languages; Java | Via Java app / LSP; not a native overlay | **BYO server** (self-host HTTP API; point clients at `localhost:8081`) — not BYO LLM key | Very active, ~15.2k★, pushed daily | Proves the **local-server + thin-client** architecture and multilingual rule packs; heavy (~GBs RAM, ~16 GB n-grams per Harper's critique) |
| **[Vale](https://github.com/vale-cli/vale)** | **MIT** | Prose **linter** (style/terminology), markup-aware, Go | CLI, any OS | N/A (no AI) | Active, ~6.3k★ | The right model for **style guides as versioned YAML** — our enterprise-style-guide substitute |
| **[textlint](https://github.com/textlint/textlint)** | **MIT** | Pluggable JS linter for prose | CLI/Node | N/A | Active, ~3.2k★ | Plugin ecosystem as distribution strategy |
| **[LTeX+](https://github.com/ltex-plus/ltex-ls-plus)** | **MPL-2.0** | LSP wrapper over LanguageTool; LaTeX/Markdown | Via editor LSP | BYO LanguageTool server | Active, ~260★ | Language-server packaging is how you get 10 editors for free |
| **[proselint](https://github.com/amperser/proselint)** | **BSD-3-Clause** | Style/usage linter, Python | CLI | N/A | Maintained, ~4.6k★ | Curated "expert advice" rule corpus is cheap and high-signal |
| **[write-good](https://github.com/btford/write-good)** | **MIT** | Naive English prose linter | CLI/Node | N/A | **Stale — last push Mar 2025**, ~5.1k★ | Cautionary tale: naive heuristics ("passive voice" regex) produce false positives and lose trust fast |
| **[alex](https://github.com/get-alex/alex)** | **MIT** | Inclusive-language linter | CLI/Node | N/A | **Stale — last push Nov 2024**, ~5.1k★ | Inclusive-language checks are a *lexicon* problem; easy to ship, easy to make embarrassing |

### 5.2 Harper Desktop is our direct prior art — study it closely

Harper now ships exactly the product shape we are considering: an **Apache-2.0, free, no-account, no-telemetry, system-wide macOS overlay grammar checker**. ([writewithharper.com/desktop](https://writewithharper.com/desktop))

Documented capabilities:

- "**Every text field. Same Harper.** If your cursor is blinking, Harper is watching. Harper Desktop hooks into macOS text inputs."
- Works in Mail, Slack, Notes, Pages, Discord, iMessage, WhatsApp, Gmail.
- "**Suggestions in under 10ms**" — no cloud round-trip.
- **Per-app enable** ("Grammar checking only where you want it").
- **Every rule is toggleable**; personal dictionary; 5 English dialects.
- **Weirpacks** — user-authored custom rules.
- macOS 14+ (Sonoma), Apple Silicon + Intel, **currently in beta**, free, no account.
- Windows/Linux: "not yet… we started on macOS to get the system-wide integration right."
- Explicit positioning: *"no cloud round-trips, no telemetry, no account, **no LLM in the loop**."*

**What this means for us — the strategic picture in one paragraph:** the "private Grammarly" position is **already occupied** by a credible, well-funded (Automattic), Apache-2.0 project that has solved the hard macOS integration problem and gives it away free. We cannot win by being "Harper but ours." But Harper has made a **philosophical choice to exclude LLMs**, which caps it at surface-level correctness — it cannot do tone rewriting, paragraph rephrasing, summarization, audience prediction, or translation, which is precisely where Grammarly makes its money ($12–30/seat). **The unoccupied gap is: Harper's local-first, system-wide macOS ergonomics + Grammarly's paid-tier rewriting quality, with the user's own key and no vendor in the middle.** Harper's existence is also a gift: it is a working reference implementation of the AX-overlay plumbing, and its README documents the exact competitor weaknesses (Grammarly's cloud dependency, LanguageTool's memory footprint) that our pitch should reuse.

### 5.3 Commercial BYOK / adjacent competitors

| Product | Model | macOS | BYOK | Notes |
|---|---|---|---|---|
| **[Kerlig](https://www.kerlig.com/)** | AI writing app, selection-triggered | **Yes** (Apple Silicon + Intel, macOS 12+) | **Yes — "connect with your preferred AI providers using API keys", 350+ models** | **Closest commercial analogue.** Built-in + custom actions (proofread, improve, summarize, shorten), MCP servers, chat-with-documents, "Agentic Mode" in 2.8. Stores messages encrypted locally. |
| **[BoltAI](https://boltai.com/)** | Native Mac AI workspace | **Yes** (macOS 13+, SwiftUI/AppKit) | **Yes — 300+ models**, incl. Ollama/local | Broad AI client with global shortcuts, agents, MCP; writing is one use case. Shows "native Mac + BYOK + model switching" is a proven, sellable shape. |
| **Apple Writing Tools / Apple Intelligence** | OS-level rewriting, proofreading, summarizing | **Yes, built in** (macOS 15.1+; macOS 27 "Golden Gate" current) | No — Apple models, on-device + Private Cloud Compute | **The real floor of the market: free and already installed.** Any product must beat "select text → Writing Tools → Proofread" on *quality and app coverage*, not on availability. ([Apple](https://support.apple.com/en-us/121115)) |
| **[ProWritingAid](https://prowritingaid.com/pricing)** | Cloud writing coach; Mac desktop app | Yes | No | $ tiered Free/Premium/Premium Pro; deep reports (line editing, pacing, dialogue) — a different, fiction-centric niche |
| **[LanguageTool Premium](https://languagetool.org/premium)** | Cloud, freemium | Yes (web/app) | Partial (self-host) | ~20k extra checks, AI paraphrasing, style guide, 150k chars |
| Sapling, Wordtune, Ginger | Cloud AI writing/rewrite | Varies | No | Adjacent; weaker macOS system-wide coverage |

**Market read:** BYOK is an established, validated pattern on macOS (Kerlig, BoltAI) and **nobody has combined it with a Harper-quality system-wide overlay for *writing specifically*.** The commercial BYOK players are *chat-first* apps with writing as a feature; the OSS player is *writing-first* with no LLM. The intersection is empty.

---

## 6. What is hard, moot, or a bad idea to copy

| Grammarly capability | Why we can't/shouldn't replicate | Realistic substitute |
|---|---|---|
| **Plagiarism detection** ("billions of web pages") | Requires a **web-scale crawl + index** and **licensed academic/professional corpora** (journals, ProQuest-style databases, student paper repositories). This is a data-licensing and infrastructure business, not a feature. Cost is permanent and ongoing (re-crawl, storage, legal). | (a) **Local overlap check against user-supplied corpora** (their own docs, a team wiki, a course reading pack) via shingling/MinHash — genuinely useful, fully private. (b) **Quote & attribution hygiene**: detect unsourced quotes/statistics and offer citation scaffolding. (c) **Optional** web-snippet check using the user's own search API key, clearly labelled "not a plagiarism database." Do **not** market it as plagiarism detection. |
| **AI detection** | Needs **proprietary labelled training data** + continuous adversarial retraining against every new model. Even the leaders are unreliable (Grammarly claims 99% / "#1 on RAID"; GPTZero was acquired in Jun 2026 to bolster this). Publishing a local detector means shipping a model that goes stale in weeks and produces **harmful false accusations** in academic settings. | (a) **"AI-ish prose" linter**: cliché/phrase list (a *lexicon* problem, like `alex`), burstiness/variance heuristics, em-dash and "delve" style tells — presented as **style feedback, not a verdict**. (b) **Authorship/provenance tracking**, below — strictly more honest and *locally computable*. (c) Ship Grammarly's own AI Rewriter concept: flag overused-AI phrases, suggest alternatives. |
| **Humanizer** | Actually **easy** with an LLM — it's a prompt. Grammarly's edge is only its evaluation loop. | Ship it: a "sound human" rewrite preset with a strong style prompt. This is a **feature-parity win at near-zero cost**. |
| **Authorship / provenance** | Grammarly relies on **clipboard access + cloud reports**. | **Fully replicable and better locally**: log keystrokes/paste events, hashes of pasted content, and AI-edit operations into a local, signed, append-only provenance log; export a report. No clipboard surveillance of *other* apps needed beyond the paste event. This is a **differentiator**, especially for education. |
| **Enterprise style-guide "ML"** | Grammarly learns org voice from usage across 50k orgs; the moat is aggregate behavioural data, not the UI. | **Rules + retrieval**: parse an uploaded style guide into **Vale-style YAML rules** for deterministic checks, plus **RAG over the style guide** for LLM-grounded rewrites. 80% of the value, 5% of the cost, and auditable — which enterprises actually prefer. |
| **Brand tones (learned)** | Trained on org writing samples. | **Few-shot voice profile**: let the user paste 3–10 sample documents → derive a reusable system prompt / style card, stored locally. Works with any BYOK model. |
| **Knowledge Share** | Needs a **company term database + admin console + per-app hover**. | Replicable: local glossary (YAML/JSON) + hover tooltips + LLM context injection. Scope to **one surface first** (our own panel) rather than 6 host apps. |
| **1M+ app coverage** | Not magic — it's the Accessibility API, and it is **fragile per-app** (see §3.6: Docs, canvas editors, secure fields, PowerPoint, long docs). | Copy the mechanism, not the claim: AX-based overlay + per-app block list + graceful degradation. **Resist publishing an app count.** |
| **AI Grader / Education suite** | Requires rubric modelling, LMS integrations (Canvas/Blackboard/Moodle), institutional sales, FERPA-grade compliance. | Out of scope. Education is a **sales and compliance** business, not a feature. |
| **Real-time inline latency at scale** | Grammarly's <100ms feel comes from datacenter models + a 17-year-old incremental engine. | **Hybrid architecture is the answer**: local rule engine (Harper/LanguageTool-class, <10ms) for correctness on every keystroke; **LLM only on demand** (selection → rewrite). Never block typing on a network call. |
| **Multilingual suggestion quality** | Grammarly built 24 language packs over years. | **Free via the LLM.** Match on day one; no rule packs to maintain. |
| **Trust/compliance surface** (SOC 2, ISO 27001, HIPAA, DLP, SCIM, audit logs) | Years of audit work; enterprise buyers require it. | Turn it into an **advantage**: no data leaves the device ⇒ smaller breach surface, no DPA needed for text, GDPR-simple. Ship a clear local-data statement instead of a compliance binder. |
| **Expert Review / human editing** | Grammarly **killed it** (Jan 2025; Expert Review Mar 2026). | Moot. Don't build a marketplace. |

**Also moot:** copying the *suggestion-category taxonomy* as a product surface. Grammarly itself is moving away from fixed categories toward **prompt-steerable agent preferences** ("tell it which underlines you care about"). We should ship **preferences-as-natural-language** from the start — it's cheaper to build than 40 toggles and it's where the incumbent is heading.

---

## 7. Decision-relevant takeaways

1. **The competitor is Superhuman, not Grammarly.** Grammarly is now one agent in a platform; enterprise is sold as "Superhuman Go." Any positioning doc that says "we're an open-source Grammarly" is describing a 2023 product.
2. **The rebrand is 2026, not 2025.** 2025 = acquisition of Superhuman; 2026 = parent company renamed Superhuman + Go launched. Correct this in our materials.
3. **The moat to attack is architectural, not feature-level.** Grammarly computes *everything* — including spelling — on its servers. "Your text never leaves this Mac, and the model is your key" is a claim it cannot copy without cannibalizing itself. Lead with it.
4. **The paywall to undercut is at clarity/tone/rewriting ($12–30/seat).** Free gets correctness; paid gets judgement. Our LLM does judgement natively — so we can give away what Grammarly charges for.
5. **Harper Desktop already owns "private, free, system-wide macOS grammar."** Do not rebuild it. Either build on `harper-core`/`harper-ls` (Apache-2.0 — permissive, compatible) for the sub-10ms local correctness layer, or differentiate on layers Harper explicitly refuses ("no LLM in the loop"). **Building on Harper's engine is the single highest-leverage architectural choice available.**
6. **Accessibility-API overlay is the only viable macOS mechanism** — confirmed by Grammarly's MDM Accessibility profile, Harper Desktop, and Kerlig. Budget for a permanent tax: per-app breakage (Docs canvas, Electron quirks, secure input fields, long docs, OS updates).
7. **Skip plagiarism and AI detection as *products*.** Both are data-licensing businesses (and Grammarly just bought GPTZero to shore its version up). Replace with local corpus overlap + AI-phrase linting + **Authorship-style provenance**, which is more honest, locally computable, and a genuine differentiator in education.
8. **Free wins available:** humanizer, translation (19 langs), multilingual suggestions (24 langs), paragraph rewrites, tone adjustment — all one prompt each. Grammarly charges for all of these.
9. **Enterprise style guide = YAML rules + RAG, not ML.** Vale proves the rule format; RAG handles the fuzzy rest; auditors prefer deterministic rules anyway.
10. **The Apple floor is real.** macOS Writing Tools is free, installed, and does proofread/rewrite/summarize. We must beat it on *quality, app coverage, and context* — not on existence.

---

## 8. Source index

**Grammarly / Superhuman primary**
- Pricing: https://www.grammarly.com/plans
- Homepage (pricing block, agents): https://www.grammarly.com/
- Enterprise: https://www.grammarly.com/business/enterprise
- Business: https://www.grammarly.com/business
- Education: https://www.grammarly.com/edu
- Desktop: https://www.grammarly.com/desktop · macOS: https://www.grammarly.com/desktop/mac
- Press/newsroom (rebrand, GPTZero, acquisitions): https://www.grammarly.com/press
- Superhuman Go: https://www.superhuman.com/go
- Superhuman platform: https://superhuman.com/superhuman-platform
- GPTZero acquisition: https://blog.superhuman.com/superhuman-to-acquire-gptzero/
- Plagiarism checker: https://www.grammarly.com/plagiarism-checker
- AI detector: https://www.grammarly.com/ai-detector
- Wikipedia (corporate history, Expert Review discontinuation): https://en.wikipedia.org/wiki/Grammarly

**Help Center (Zendesk API, `support.grammarly.com/api/v2/help_center`)**
- Go for Windows/Mac user guide (macOS UI, prompts/day, Word-only plagiarism): `45113077035789`
- Grammarly Win/Mac user guide (widget, tone, shortcuts): `4412816078349`
- Grammarly ↔ Go coexistence: `45976306096781`
- Choose where it works / block list: `44208969169037`
- System requirements: `44209059689869`
- Keyboard navigation & shortcuts: `24059143083277`
- Deploy via Jamf (DMG, bundle id, Accessibility profile): `8341875702413`
- Google Docs support: `115000090991` · Go in Docs: `45112868279821`
- Known issues on websites: `45183836343437`
- Docs / AI writing surface (agent list): `38552281546765`
- Product offerings: `115000090851`
- Grammarly Pro (Premium vs Pro, seats): `25043801131149`
- Authorship: `29548735595405`
- Knowledge Share: `16664924710797`
- Snippets: `4403077145485`
- Brand tones: `4403544890253`
- Personal dictionary: `115000091491` · Org dictionary: `14441067751309`
- Language support: `115000090971` · Translation: `27518589299725`
- AI Rewriter: `42141090078861`
- Customize the Grammarly agent: `48202938578317`
- Privacy & security FAQ (AWS us-east, BYOK encryption): `20916119474829`
- Word-on-Mac discontinuation: `39667635821325` · Office add-in retirement: `39983075497485` · Expert writing service ended: `34083794138381`

**Open source**
- Harper: https://github.com/Automattic/harper · https://writewithharper.com/desktop · https://writewithharper.com/
- LanguageTool: https://github.com/languagetool-org/languagetool · https://languagetool.org/premium
- Vale: https://github.com/vale-cli/vale
- textlint: https://github.com/textlint/textlint
- LTeX+: https://github.com/ltex-plus/ltex-ls-plus
- proselint: https://github.com/amperser/proselint
- write-good: https://github.com/btford/write-good
- alex: https://github.com/get-alex/alex

**Commercial / platform**
- Kerlig: https://www.kerlig.com/
- BoltAI: https://boltai.com/
- ProWritingAid: https://prowritingaid.com/pricing
- Apple Intelligence requirements / Writing Tools: https://support.apple.com/en-us/121115

---

## 9. Open questions this audit could not close

1. **Exact rebrand date** (month/day) — press page lists the release without a date; news coverage would settle it. `web_search` was unavailable.
2. **Superhuman Go / Mail / Docs list pricing** — page is JS-rendered; no static numbers. Needs a browser or a rep.
3. **Grammarly Enterprise per-seat price** — contact-sales only. Public estimates exist in press coverage; unverified here.
4. **Education per-seat pricing** and minimum seat counts.
5. **Independent verification of "99% accuracy / #1 on RAID"** and of the "1M+ apps" claim.
6. **Whether Grammarly does any on-device inference** — no documentation found either way; the architecture strongly implies none.
7. **Harper Desktop's AX implementation details** (how it handles secure input, canvas editors, Electron) — worth reading its source before we design ours.
