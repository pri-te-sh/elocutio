<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/lockup-white.png" />
    <img src="assets/brand/lockup-indigo.png" alt="Elocutio" width="420" />
  </picture>
  <h3>Your voice. Clearly expressed.</h3>
  <p>A private, open-source writing companion for macOS — with Windows, browser, iOS, and Android on the roadmap.</p>
  <p>
    <a href="https://github.com/pri-te-sh/elocutio/actions/workflows/ci.yml"><img src="https://github.com/pri-te-sh/elocutio/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License: Apache-2.0"></a>
    <a href="#requirements"><img src="https://img.shields.io/badge/platform-macOS%2014%2B-lightgrey.svg" alt="Platform: macOS 14+"></a>
    <a href="ROADMAP.md"><img src="https://img.shields.io/badge/status-pre--alpha-orange.svg" alt="Status: pre-alpha"></a>
    <a href="CONTRIBUTING.md"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs welcome"></a>
  </p>
</div>

---

**Elocutio** (el-oh-KOO-tee-oh) is a private writing companion that refines clarity, correctness, and tone while preserving your voice. It starts on macOS, where it lives in your menu bar and improves the text you are already writing — in Mail, Slack, Notes, Word, or your browser. Local rules catch the obvious mistakes instantly; your chosen model handles judgment: clarity, tone, rewrites, compose, translate, and humanize. Over time it learns **your voice** from the writing you upload and the edits you accept, so rewrites sound like *you*. You bring the API key; your text goes only to the provider you configured.

> **Status: pre-alpha / M1 in progress.** The macOS app builds and runs on a rebranded [Harper](https://github.com/Automattic/harper) fork (Apache-2.0). Local checks and a floating suggestion card work in TextEdit, Notes, and Safari. OpenRouter is the default provider; OpenAI-compatible endpoints (OpenAI, Ollama, LM Studio) are supported, and a curated model catalog backs the picker. Structured LLM suggestions merge into the inherited card with a ranked, explained merge; **⌥⌘E** rewrites a selection, **⌥⌘A** analyzes the focused field, and **⌥⌘Z** undoes the last Elocutio edit. A local voice style card learns from 300+ word samples and, when opted in, from the edits you accept, dismiss, or change — with a copy guard that refuses any rewrite repeating your own samples. Tone detection runs entirely on your Mac. Chrome/Mail overlay geometry and a signed/notarized release remain open. See the [roadmap](ROADMAP.md), [build plan](docs/PLAN.md), and [progress dashboard](docs/PROGRESS.md). Expect breaking changes; there is no signed release yet.

## Why Elocutio

Grammarly is excellent, but it is closed, subscription-based, and cloud-only. In 2026, three things have changed:

1. **Writing quality is available via API.** A cheap model can proofread, rewrite, and explain edits at a quality that was impossible a few years ago.
2. **Local models are good enough for short text.** Harper gives sub-10 ms English checks on-device; Apple's Foundation Model and Ollama cover privacy-first use.
3. **People want control.** Which model, which provider, what leaves the machine, and what it costs should be the user's decision — not a subscription tier.

Grammarly has no documented offline mode: its clients send text to the cloud for analysis. Its enterprise "BYOK" means bring-your-own-**encryption**-key, not bring-your-own-model. Elocutio's premise is the opposite: **local-first, model-agnostic, no vendor in the middle.**

## What it will do

| Area | Capability |
| --- | --- |
| **Proofreading** | Spelling, grammar, punctuation, capitalization, commonly confused words, agreement, sentence structure |
| **Clarity** | Conciseness, wordiness, redundancy, passive voice, readability, jargon |
| **Tone** | Tone detection, formality/confidence/friendliness dials, tone rewrites, inclusive language |
| **Rewrites** | Sentence and paragraph rewrites, custom instructions, paraphrase modes, humanizer |
| **Generative** | Compose, reply, summarize, outline, translate, explain |
| **Voice** | Learns your rhythm, diction, punctuation, greetings, sign-offs, rhetorical moves, and signature phrases from uploads and feedback; per-context profiles; encrypted, local samples |
| **Assistant** | Context-aware chat panel with explicit context chips |
| **Personalization** | Personal dictionary, style guide, brand voice, glossary, snippets, per-app preferences |
| **Everywhere** | macOS menu-bar app with inline suggestions in native apps; Chrome/Edge extension for web editors; Windows, iOS, and Android next |
| **Control** | Per-app on/off, local-only mode, provider routing, model picker, cost meter, network log |

**Not in v1:** Google Docs (canvas), plagiarism detection, AI-detector scores, enterprise SSO/SCIM/DLP, Safari extension. **Windows, iOS, and Android are planned after the macOS release**, and the shared Rust core is designed for them from day one. See the [feature matrix](docs/FEATURE-MATRIX.md) and [platform strategy](#platforms).

## How it works

```text
┌──────────────────── Tauri main process (Rust) ─────────────────────┐
│  tray · updater · config · SQLite · Keychain                       │
│  providers (OpenRouter/OpenAI/Anthropic/Gemini/Ollama)             │
│  analysis: chunk → prompt → validate → resolve offsets → dedupe    │
│  voice: samples → style card → exemplars → prompt                  │
│  bridge: 127.0.0.1 WebSocket + pairing token                       │
└───────────────────────────────────┬────────────────────────────────┘
                                    │ IPC (JSON lines)
┌───────────────────────────────────▼────────────────────────────────┐
│  Highlighter subprocess (Rust + native macOS)                      │
│  Accessibility read · Harper local rules · overlay underlines      │
│  suggestion card/widget (non-activating) · paste/AX injection      │
└────────────────────────────────────────────────────────────────────┘
```

- The release target is non-sandboxed, Developer ID-signed, and notarized. The Mac App Store is not an option because App Sandbox cannot read other apps' text via Accessibility. Current builds are unsigned developer builds.
- Reads use the Accessibility API. Writes use a ladder: AX fast path in AppKit, **pasteboard + ⌘V** in Chromium/Electron, with read-back verification.
- The card never takes keyboard focus. Focus stealing breaks the paste and the product.
- Google Docs is explicitly unsupported (canvas rendering); Gmail, Notion, Slack, and Linear are covered by the extension.
- Voice samples are analyzed locally and discarded; only the aggregate style card is stored. Accepted-edit feedback is local and opt-in. The style card is sent to your provider only when you enable voice-aware rewrites.

The core is shared Rust; platform shells are thin. More detail: [architecture](docs/ARCHITECTURE.md) · [voice design](docs/VOICE-LEARNING.md) · [brand](docs/BRAND.md) · [privacy](docs/PRIVACY.md).

## Providers

OpenRouter is the default because one key reaches many models. Settings → Provider can switch to any OpenAI-compatible Chat Completions endpoint (OpenAI, Ollama, LM Studio) with a custom base URL and optional key. Direct Anthropic/Google adapters and Apple's on-device model are planned; Claude and Gemini models are available through OpenRouter.

| Task | Default | Escalation |
| --- | --- | --- |
| Proofread / clarity | `anthropic/claude-haiku-5.5` | `claude-sonnet-5` |
| Tone detection | `anthropic/claude-haiku-5.5` | — |
| Rewrite / compose / translate | `anthropic/claude-haiku-5.5` | `claude-sonnet-5` → `gpt-5.6-sol` |
| Budget mode | `deepseek/deepseek-v4.1-flash` | — |
| Local | Apple Foundation Model (macOS 26) or Ollama | — |

No markup, no credits, no middleman. You pay your provider directly. At October 2026 prices, the default costs roughly **$0.28 per 1,000 email reviews** and **$1.06 per 1,000 1,500-word documents**. Local mode is free.

## Voice learning

Elocutio does not fine-tune a model on your writing. It builds a local, explainable **Voice Profile**:

- **Uploads** — paste 300+ words in Settings → Voice; the sample is analyzed locally and discarded.
- **Behavior** — accepted edits are recorded locally only when **Learn from my accepted edits** is enabled.
- **Applied** — a compact style card is added to rewrite prompts when **Use my voice** is enabled.
- **Future** — dismissed/edited-after-accept weighting, "sounds like me" ratings, trait overrides, and exemplar retrieval.

Two independent switches, both **off by default**: *Learn from my edits* and *Use my voice*. Only the aggregate style card is stored; raw samples are never written. See [VOICE-LEARNING.md](docs/VOICE-LEARNING.md).

## Roadmap

| Phase | Status | Outcome |
| --- | --- | --- |
| Planning | ✅ Done | Audit, plan, architecture, feature matrix, ADRs, prompt library, voice design |
| **M0 — Fork & spike** | 🚧 Partial | Fork/rebrand Harper v2.13.0; TextEdit, Notes, and Safari overlay; OpenRouter validation; system-wide rewrite. Chrome/Mail overlay still open. |
| **M1 — LLM MVP** | 🚧 In progress | Local + LLM suggestions, inherited card, accept/dismiss, floating widget, Keychain, usage log, budget warning, cost cap, undo, provider adapters and model catalog, golden-set harness, onboarding |
| **M2 — Inline + voice v0** | 🚧 Started | Widget count/AI action, local tone detection, style card, accepted/dismissed feedback, copy guard, trait overrides, ratings, live-mode throttle, block list; diff rewrite, custom instructions, 10-app matrix remain |
| M3 — Everywhere + uploads | Planned | Chrome/Edge extension, assistant panel, compose/reply/summarize, voice uploads |
| M4 — Personal + voice v2 | Planned | Style guide, brand voice, glossary, snippets, provenance, history; per-context voices |
| M5 — v1.0 | Planned | Notarized DMG, signed updates, Homebrew, docs, security review |

Full roadmap: [ROADMAP.md](ROADMAP.md). Detailed scope and estimates: [docs/PLAN.md](docs/PLAN.md). Task-level execution status, evidence, and acceptance criteria: [docs/PROGRESS.md](docs/PROGRESS.md).

## Platforms

| Platform | Status | Approach |
| --- | --- | --- |
| **macOS** | **v1 target** | Tauri + Harper Accessibility overlay, menu-bar agent |
| **Chrome/Edge** | M3 | Manifest V3 extension + local bridge |
| **Windows** | After v1 | Tauri + UI Automation/Win32; shared Rust core |
| **iOS** | Planned | Native app + keyboard/share extensions; no cross-app accessibility |
| **Android** | Planned | Native app + AccessibilityService and/or IME; shared Rust core |
| **Safari/Firefox** | Later | WebExtension adaptation; Safari inside the notarized app |

The shared crates (`elocutio-core`, `elocutio-analysis`, `elocutio-llm`, `elocutio-voice`, `elocutio-store`, `elocutio-bridge`) are platform-agnostic. Each platform gets a thin shell for text capture, overlays, injection, and distribution. See [ADR-021](docs/DECISIONS.md).

## Requirements

- macOS 14 Sonoma or later for the v1 app (Windows, iOS, and Android are planned)
- Accessibility permission on macOS (required for cross-app text access)
- Optional: Screen Recording (OCR fallback only)
- An API key for a supported provider, or a local model
- For development: Xcode Command Line Tools, Rust 1.98+, Node.js 22+, pnpm 11+, Python 3.11+ (docs and progress scripts)

## Development

The macOS app builds from this repository and runs as a menu-bar agent. `just macos-release` produces `Elocutio.app` and a DMG under `apps/macos/src-tauri/target/release/bundle/macos/`. The app is ad-hoc signed but not notarized; Gatekeeper will require right-click → Open or a local build until the M5 Developer ID release. The shared Rust crates, docs, and open-source tooling are included.

```bash
git clone git@github.com:pri-te-sh/elocutio.git
cd elocutio
./scripts/bootstrap.sh   # check tooling, install JS deps, run tests
just check               # fmt + clippy + tests + docs + prettier
just test                # Rust tests only
just fetch-upstream      # shallow-clone Harper v2.13.0 into .upstream/
just assets              # regenerate app/platform icons from brand sources
just macos-check         # type-check the macOS frontend + Tauri backend
just macos-build         # build an unsigned debug Elocutio.app
just macos-release       # build an optimized unsigned app + DMG
just macos-dev           # run the macOS shell in development mode
just dev-cert            # create a stable self-signed cert for TCC
just progress            # build the static progress dashboard
just progress-serve      # live progress dashboard at http://127.0.0.1:8787
```

If you do not have `just`, run the commands directly:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
python3 scripts/check_docs.py
python3 scripts/progress.py validate
python3 -m unittest discover -s scripts -p "test_*.py"
pnpm format:check
```

## Build progress

Milestones, work items, and acceptance criteria are tracked in
[`progress/tracker.json`](progress/tracker.json). Most statuses are derived
automatically from repository evidence — files, globs, text markers, git
remotes, and commit subjects — so the dashboard updates as code lands. Manual
overrides are supported and always win.

`just progress` writes a static snapshot to `progress/out/dashboard.html`;
`just progress-serve` runs the live dashboard with filters, git activity, and
inline status editing. See [docs/PROGRESS.md](docs/PROGRESS.md) for the ledger
schema, evidence rules, and API.

## Repository layout

```text
.
├── assets/
│   ├── brand/               # logo sources, lockups, tokens, brand guide
│   └── icons/               # generated app/menu-bar/platform icons
├── crates/
│   ├── elocutio-core/       # domain types, suggestion model
│   ├── elocutio-analysis/   # span resolution, chunking (growing)
│   ├── elocutio-llm/        # provider-agnostic request/response types
│   ├── elocutio-voice/      # local voice profiles and style cards
│   ├── elocutio-store/      # SQLite schema and repositories (growing)
│   ├── elocutio-bridge/     # browser-extension protocol
│   └── elocutio-platform/   # shared capture/injection/overlay traits
├── apps/macos/              # forked Harper macOS shell (M0, buildable)
├── vendor/harper/           # vendored harper-core subset (Apache-2.0)
├── apps/windows/            # Windows shell (post-v1)
├── apps/ios/                # iOS app + keyboard extension (planned)
├── apps/android/            # Android app + accessibility service (planned)
├── extensions/chrome/       # MV3 extension (M3)
├── docs/                    # audit, plan, architecture, prompts, privacy
├── research/                # source material and citations
├── scripts/                 # bootstrap, docs check, signing, upstream fetch, progress tracker
├── progress/                # build-progress ledger, dashboard template, snapshots
├── .github/                 # CI, release placeholder, templates, Dependabot
├── Cargo.toml               # Rust workspace
├── justfile                 # developer tasks
└── README.md
```

## Documentation

- **[Executive brief](docs/EXECUTIVE-BRIEF.md)** — the two-page version: findings, scope, timeline, costs, decisions.
- **[Product audit](docs/AUDIT.md)** — what Grammarly/Superhuman ships in 2026, what is replicable, what is not, and where the gap is.
- **[Build plan](docs/PLAN.md)** — strategy, scope, UX, milestones, testing, distribution, risks.
- **[M0 verification](docs/M0-VERIFICATION.md)** — build evidence, overlay matrix, OpenRouter check, and known gaps.
- **[Progress tracker](docs/PROGRESS.md)** — evidence-derived milestone status, the live dashboard, and how to update the ledger.
- **[Architecture](docs/ARCHITECTURE.md)** — process model, modules, data types, provider layer, macOS integration, IPC, store, security.
- **[Feature matrix](docs/FEATURE-MATRIX.md)** — 94 features with parity target, approach, phase, effort, and risk.
- **[Decision records](docs/DECISIONS.md)** — the choices that shape the build, with alternatives.
- **[Voice learning](docs/VOICE-LEARNING.md)** — how the voice profile works, why it is not fine-tuning, and the privacy model.
- **[Prompt library](docs/PROMPTS.md)** — system prompts, JSON schemas, token budgets, and evaluation rules.
- **[Privacy](docs/PRIVACY.md)** — data flows, permissions, retention, and verifiable claims.
- **[Testing](docs/TESTING.md)** — unit, integration, manual matrix, and quality gates.
- **[Brand](docs/BRAND.md)** — logo system, palette, typography, voice, and platform icon assets.
- **[Maintaining](docs/MAINTAINING.md)** — GitHub settings, labels, triage, CI, security, and release checklist.
- **[Research notes](research/)** — sources, citations, and the raw OpenRouter model analysis.

## Contributing

Elocutio is early, and the highest-leverage contributions are product feedback, platform testing, and prompt/evaluation work — not just code. See [CONTRIBUTING.md](CONTRIBUTING.md).

- Open an issue before a large pull request.
- Good first issues are labelled `good first issue`.
- Commits must be signed off (`git commit -s`) under the [DCO](https://developercertificate.org/).
- By contributing, you agree your contribution is licensed under Apache-2.0.
- Follow the [Code of Conduct](CODE_OF_CONDUCT.md).
- Report vulnerabilities privately per [SECURITY.md](SECURITY.md), never in a public issue.

## License and attribution

Elocutio is licensed under **Apache-2.0**. It is derived from [Harper](https://github.com/Automattic/harper), Copyright (c) Automattic, also Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Elocutio is not affiliated with or endorsed by Automattic.

## Trademark

Elocutio is not affiliated with, endorsed by, or sponsored by Grammarly, Inc., Superhuman, or Automattic. "Grammarly" and "Superhuman" are trademarks of their owners and are used here only for descriptive comparison. The project does not use their logos or visual assets. The Elocutio name is provisional pending formal trademark clearance.
