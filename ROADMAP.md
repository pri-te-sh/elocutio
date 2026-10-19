# Roadmap

Elocutio is pre-alpha. The detailed scope and acceptance criteria live in
[`docs/PLAN.md`](docs/PLAN.md); this is the short version.

| Phase | Status | Outcome |
| --- | --- | --- |
| **Planning** | ✅ Done | Audit, plan, architecture, feature matrix, ADRs, prompt library, voice design |
| **M0 — Fork & spike** | 🚧 Next | Fork/rebrand Harper v2.13.0, verify the overlay in 5 apps, validate OpenRouter, prove a selection rewrite |
| **M1 — LLM MVP** | Planned | Local + LLM suggestions, card, accept/dismiss, tone detection, onboarding, Keychain, usage log |
| **M2 — Inline + voice v0** | Planned | Widget, underlines, tone/paragraph rewrites, cache, blocklist; voice profile from feedback |
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
