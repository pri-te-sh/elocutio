# Contributing to Elocutio

Thanks for being here. Elocutio is an early, open-source project, and the highest-leverage contributions are **product feedback, platform testing, prompts/evaluation, and docs** — not only code.

> **Status: pre-alpha.** The desktop app is being scaffolded on a fork of Harper. Expect the architecture and interfaces to move. For anything larger than a small fix, open an issue first so we can agree on the approach.

## Ways to help

- **Product feedback:** which apps, writing tasks, and tones matter most to you?
- **Platform testing:** try builds in Mail, Word, Pages, Slack, Chrome, Notion, Obsidian, and tell us what breaks.
- **Voice testing:** upload a few writing samples and tell us whether the style card sounds like you.
- **Rule packs:** style-guide YAML, glossaries, and AI-phrase lists.
- **Prompts and evaluation:** golden-set items, prompt improvements with measured results.
- **Code:** Rust (providers, analysis, macOS platform), TypeScript/Svelte (UI, extension), docs.
- **Docs:** fixes, screenshots, tutorials, and translations.

Look for issues labelled `good first issue` and `help wanted`.

## Development setup

Prerequisites:

- macOS 14+ with Xcode Command Line Tools
- Full Xcode (recommended; required for signing/notarization)
- Rust 1.98+ via rustup (the pinned version is in `rust-toolchain.toml`)
- Node.js 22+ and pnpm 11+
- Tauri 2 prerequisites: <https://tauri.app/start/prerequisites/>
- `just` (optional, recommended: `brew install just`)

```bash
git clone git@github.com:pri-te-sh/elocutio.git
cd elocutio
./scripts/bootstrap.sh   # check tooling, install JS deps, run the tests
just check               # fmt + clippy + tests + docs + prettier
```

Useful tasks:

| Command | What it does |
| --- | --- |
| `just bootstrap` | Check tooling, install dependencies, run tests |
| `just check` | Every check CI runs |
| `just test` | Rust tests |
| `just fmt` | Format Rust and JS/JSON/YAML |
| `just build` | Build the Rust workspace |
| `just fetch-upstream` | Shallow-clone Harper v2.13.0 into `.upstream/` |
| `just dev-cert` | Create a stable self-signed certificate for TCC |

Without `just`:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
python3 scripts/check_docs.py
pnpm format:check
```

The desktop app arrives in M0. Once it does, macOS will prompt for Accessibility; development builds must use the stable self-signed certificate so TCC does not reset on every rebuild.

## Repository layout

```text
crates/elocutio-core       domain types and the suggestion model
crates/elocutio-analysis   span resolution and chunking
crates/elocutio-llm        provider-agnostic request/response types
crates/elocutio-voice      local voice profiles and style cards
crates/elocutio-store      SQLite schema and repositories
crates/elocutio-bridge     browser-extension protocol
apps/desktop               forked Harper desktop shell (M0)
extensions/chrome          Manifest V3 extension (M3)
docs/                      audit, plan, architecture, prompts, privacy
research/                  source material and citations
scripts/                   bootstrap, docs check, signing, upstream fetch
```

## Code guidelines

- **Rust:** `cargo fmt`, `cargo clippy -- -D warnings`, typed errors instead of `unwrap()` in library code.
- **TypeScript/Svelte:** strict mode, no `any` unless justified.
- Keep provider-specific logic inside `elocutio-llm` adapters; the analysis pipeline must be provider-agnostic.
- Never log user text, API keys, voice samples, or full provider payloads. Log hashes, token counts, and status.
- New user-visible behavior needs a test and a docs update.
- Do not add a dependency without checking its license (Apache-2.0-compatible) and maintenance status.
- Prefer small, reviewable changes over large rewrites.

## Prompt changes

Prompts are code:

1. Bump `prompt_version` in the affected task.
2. Run the golden set (the eval harness lands in M1).
3. Include before/after metrics (span F0.5, resolution rate, over-edit rate, cost) in the PR.
4. Do not merge a prompt change that drops F0.5 by more than 2 points or resolution by more than 5 points without discussion.

## Commits and pull requests

- Small, focused commits with a clear message.
- One logical change per PR; include a test plan.
- Link the issue; describe user-visible changes and risks.
- Add or update tests and docs.
- Sign off your commits: `git commit -s` (DCO). By contributing, you agree your contribution is licensed under Apache-2.0 and that you have the right to submit it.
- UI changes need a screenshot or short recording.
- Never include API keys, private text, or voice samples in a PR or issue.

## Maintainer docs

Repository settings, labels, triage, and release steps are in [docs/MAINTAINING.md](docs/MAINTAINING.md).

## Upstream (Harper) changes

Elocutio is a fork of Harper. Keep patches small and documented in [`UPSTREAM.md`](UPSTREAM.md). If a fix is generic (Accessibility, overlay geometry, highlighter), consider sending it upstream first — it reduces our merge burden and supports the project we depend on.

## Code of conduct

All participation is covered by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Security

Do not report vulnerabilities in public issues. See [SECURITY.md](SECURITY.md).
