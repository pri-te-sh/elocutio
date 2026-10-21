# Roadmap

Elocutio is pre-alpha. The detailed scope and acceptance criteria live in
[`docs/PLAN.md`](docs/PLAN.md); this is the short version. Task-level execution
status, evidence, and the live dashboard live in
[`docs/PROGRESS.md`](docs/PROGRESS.md) and [`progress/tracker.json`](progress/tracker.json).

| Phase | Status | Outcome |
| --- | --- | --- |
| **Planning** | ✅ Done | Audit, plan, architecture, feature matrix, ADRs, prompt library, voice design |
| **M0 — Fork & spike** | 🚧 Partial | Rebranded app builds; TextEdit/Notes/Safari pass the overlay; OpenRouter key works; the system-wide rewrite works in TextEdit, Safari, and Chrome. The Chrome/Mail overlay and the inherited security audit remain. |
| **M1 — LLM MVP** | 🚧 In progress | Local + LLM suggestions, card, accept/dismiss, onboarding, Keychain, usage log. Tone detection, the provider abstraction, and the golden-set harness are landing now. |
| **M2 — Inline + voice v0** | 🚧 Started | Widget count/AI action, tone presets, style card, accepted-edit learning; live mode, diff rewrite, the voice copy guard, and the 10-app matrix remain. |
| **M3 — Everywhere + uploads** | Planned | Chrome/Edge extension, assistant panel, compose/reply/summarize, multi-provider; voice sample uploads |
| **M4 — Personal + voice v2** | Planned | Style guide, brand voice, glossary, snippets, provenance, history; per-context voices |
| **M5 — v1.0** | Planned | Notarized DMG, signed updates, Homebrew, docs, security review |

## Platforms

macOS is the first target. The shared Rust core is designed for the rest from day one.

| Platform | Status | Notes |
| --- | --- | --- |
| **macOS** | v1 target | Menu-bar app, inline suggestions, Accessibility overlay |
| **Chrome/Edge** | M3 | Gmail, Notion, Slack, Linear; no Google Docs |
| **Windows** | After v1 | Tauri + UI Automation; signed installer |
| **iOS** | Planned | Keyboard + share extensions; no cross-app access |
| **Android** | Planned | IME and/or AccessibilityService; Play policy review |
| **Safari/Firefox** | Later | WebExtension adaptation |

## Not in v1

Google Docs (canvas), plagiarism detection, AI-detector scores, enterprise
SSO/SCIM/DLP, Safari extension, cloud sync, cloud fine-tuning of user writing.
Windows, iOS, and Android are planned after the macOS release.

## Brand and identity

The name, tagline, palette, typography, logo rules, and platform icons are in
[`docs/BRAND.md`](docs/BRAND.md). The source artwork is in `assets/brand/source/`;
regenerate platform icons with `python3 scripts/build-assets.py`.

## How to influence the roadmap

- 👍 React to issues that matter to you.
- Open a feature request with the problem, not just the solution.
- Join the discussion on the pinned roadmap issue (once Discussions is enabled).
