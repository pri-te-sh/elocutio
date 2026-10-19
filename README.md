<div align="center">
  <img src="assets/logo.svg" alt="Elocutio" width="380" />
  <h3>Your voice, refined.</h3>
  <p>An open-source, bring-your-own-key writing assistant for macOS.</p>
  <p>
    <a href="https://github.com/pri-te-sh/elocutio/actions/workflows/ci.yml"><img src="https://github.com/pri-te-sh/elocutio/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License: Apache-2.0"></a>
    <a href="#requirements"><img src="https://img.shields.io/badge/platform-macOS%2014%2B-lightgrey.svg" alt="Platform: macOS 14+"></a>
    <a href="ROADMAP.md"><img src="https://img.shields.io/badge/status-pre--alpha-orange.svg" alt="Status: pre-alpha"></a>
    <a href="CONTRIBUTING.md"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs welcome"></a>
  </p>
</div>

---

**Elocutio** (el-oh-KOO-tee-oh) lives in your menu bar and improves the text you are already writing — in Mail, Slack, Notes, Word, your browser, or anywhere else with a text field. Local rules catch the obvious mistakes instantly. Your chosen model handles judgment: clarity, tone, rewrites, compose, translate, and humanize. Over time it learns **your voice** from the writing you upload and the edits you accept, so rewrites sound like *you*. You bring the API key; your text goes only to the provider you configured.

> **Status: pre-alpha.** The audit, plan, architecture, and repository scaffold are in place. The desktop app is being built on a fork of [Harper](https://github.com/Automattic/harper) (Apache-2.0). See the [roadmap](ROADMAP.md) and [build plan](docs/PLAN.md). Expect breaking changes; there is no installable release yet.

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
| **Everywhere** | Menu-bar app with inline suggestions in native apps; Chrome/Edge extension for web editors |
| **Control** | Per-app on/off, local-only mode, provider routing, model picker, cost meter, network log |

**Not in v1:** Google Docs (canvas), plagiarism detection, AI-detector scores, enterprise SSO/SCIM/DLP, Windows, mobile, Safari extension. See the [feature matrix](docs/FEATURE-MATRIX.md) for the honest parity plan.

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

- The app is non-sandboxed, Developer ID-signed, and notarized. The Mac App Store is not an option because App Sandbox cannot read other apps' text via Accessibility.
- Reads use the Accessibility API. Writes use a ladder: AX fast path in AppKit, **pasteboard + ⌘V** in Chromium/Electron, with read-back verification.
- The card never takes keyboard focus. Focus stealing breaks the paste and the product.
- Google Docs is explicitly unsupported (canvas rendering); Gmail, Notion, Slack, and Linear are covered by the extension.
- Voice samples stay local and encrypted; only the style card and selected excerpts are sent, with a preview.

More detail: [architecture](docs/ARCHITECTURE.md) · [voice design](docs/VOICE-LEARNING.md) · [privacy](docs/PRIVACY.md).

## Providers

OpenRouter is the default because one key reaches many models. Direct providers, OpenAI-compatible endpoints, local Ollama, and Apple's on-device model are planned.

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

- **Uploads** — `.txt`, `.md`, `.docx`, or paste in v1; PDF/RTF/email later.
- **Behavior** — accepted, dismissed, and edited suggestions, weighted by category.
- **Explicit feedback** — "sounds like me" ratings, trait overrides, preferred/avoided terms.
- **Applied through retrieval** — a style card plus 2–4 representative excerpts, injected into rewrites and drafts.

Two independent switches, both **off by default**: *Learn from my edits* and *Use my voice*. Samples are encrypted at rest; only the card and excerpts leave the Mac, and a "style card only" mode sends no excerpts. An 8-gram copy guard prevents the model from copying sample text. See [VOICE-LEARNING.md](docs/VOICE-LEARNING.md).

## Roadmap

| Phase | Status | Outcome |
| --- | --- | --- |
| Planning | ✅ Done | Audit, plan, architecture, feature matrix, ADRs, prompt library, voice design |
| **M0 — Fork & spike** | 🚧 Next | Fork/rebrand Harper v2.13.0, verify the overlay in 5 apps, validate OpenRouter, prove a selection rewrite |
| M1 — LLM MVP | Planned | Local + LLM suggestions, card, accept/dismiss, tone detection, onboarding, Keychain, usage log |
| M2 — Inline + voice v0 | Planned | Widget, underlines, rewrites, cache, blocklist; voice profile from feedback |
| M3 — Everywhere + uploads | Planned | Chrome/Edge extension, assistant panel, compose/reply/summarize, multi-provider; voice uploads |
| M4 — Personal + voice v2 | Planned | Style guide, brand voice, glossary, snippets, provenance, history; per-context voices |
| M5 — v1.0 | Planned | Notarized DMG, signed updates, Homebrew, docs, security review |

Full roadmap: [ROADMAP.md](ROADMAP.md). Detailed scope and estimates: [docs/PLAN.md](docs/PLAN.md).

## Requirements

- macOS 14 Sonoma or later
- Accessibility permission (required for cross-app text access)
- Optional: Screen Recording (OCR fallback only)
- An API key for a supported provider, or a local model
- For development: Xcode Command Line Tools, Rust 1.98+, Node.js 22+, pnpm 11+

## Development

The desktop app is not buildable yet; M0 adds the Harper fork. The current repository contains the planning docs, shared Rust crates, and open-source tooling.

```bash
git clone git@github.com:pri-te-sh/elocutio.git
cd elocutio
./scripts/bootstrap.sh   # check tooling, install JS deps, run tests
just check               # fmt + clippy + tests + docs + prettier
just test                # Rust tests only
just fetch-upstream      # shallow-clone Harper v2.13.0 into .upstream/
just dev-cert            # create a stable self-signed cert for TCC
```

If you do not have `just`, run the commands directly:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
python3 scripts/check_docs.py
pnpm format:check
```

## Repository layout

```text
.
├── assets/                  # logo and menu-bar mark
├── crates/
│   ├── elocutio-core/       # domain types, suggestion model
│   ├── elocutio-analysis/   # span resolution, chunking (growing)
│   ├── elocutio-llm/        # provider-agnostic request/response types
│   ├── elocutio-voice/      # local voice profiles and style cards
│   ├── elocutio-store/      # SQLite schema and repositories (growing)
│   └── elocutio-bridge/     # browser-extension protocol
├── apps/desktop/            # forked Harper desktop shell (M0)
├── extensions/chrome/       # MV3 extension (M3)
├── docs/                    # audit, plan, architecture, prompts, privacy
├── research/                # source material and citations
├── scripts/                 # bootstrap, docs check, signing, upstream fetch
├── .github/                 # CI, release placeholder, templates, Dependabot
├── Cargo.toml               # Rust workspace
├── justfile                 # developer tasks
└── README.md
```

## Documentation

- **[Executive brief](docs/EXECUTIVE-BRIEF.md)** — the two-page version: findings, scope, timeline, costs, decisions.
- **[Product audit](docs/AUDIT.md)** — what Grammarly/Superhuman ships in 2026, what is replicable, what is not, and where the gap is.
- **[Build plan](docs/PLAN.md)** — strategy, scope, UX, milestones, testing, distribution, risks.
- **[Architecture](docs/ARCHITECTURE.md)** — process model, modules, data types, provider layer, macOS integration, IPC, store, security.
- **[Feature matrix](docs/FEATURE-MATRIX.md)** — 94 features with parity target, approach, phase, effort, and risk.
- **[Decision records](docs/DECISIONS.md)** — the choices that shape the build, with alternatives.
- **[Voice learning](docs/VOICE-LEARNING.md)** — how the voice profile works, why it is not fine-tuning, and the privacy model.
- **[Prompt library](docs/PROMPTS.md)** — system prompts, JSON schemas, token budgets, and evaluation rules.
- **[Privacy](docs/PRIVACY.md)** — data flows, permissions, retention, and verifiable claims.
- **[Testing](docs/TESTING.md)** — unit, integration, manual matrix, and quality gates.
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
