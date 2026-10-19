# Upstream: Harper

Elocutio is a fork of [Harper](https://github.com/Automattic/harper), licensed under Apache-2.0. The project repository is <https://github.com/pri-te-sh/elocutio>.

## Base

| Field | Value |
| --- | --- |
| Upstream | `https://github.com/Automattic/harper` |
| Base tag | `v2.13.0` (16 October 2026) |
| Base commit | `f4ebc61d77df1fc00b6aa8bc604e91af3534dad1` (recorded in `upstream.lock`) |
| Development checkout | `.upstream/harper` (gitignored; run `./scripts/fetch-upstream.sh`) |
| Local remote | `origin` (our repository) |
| Upstream remote | `upstream` |

## What we reuse

- `harper-core` and its path dependencies (local English grammar engine).
- `harper-desktop`: macOS Accessibility broker, highlighter, tray, config, updater, Tauri shell.
- Selected platform abstractions (`os_broker`, `windows_broker`) for a future Windows port.

## What we change

- Rename product, bundle ID, config paths, icons, URLs, updater keys.
- Add `elocutio-llm`, `elocutio-analysis`, `elocutio-store`, `elocutio-bridge`, the Svelte UI, and the Chrome extension.
- Extend the highlighter/IPC to render and apply LLM suggestions.
- Harden the inherited network/IPC/updater surface.

## Sync policy

1. **Pin, don't track.** We build from the tagged base, not `main`.
2. **Small patches.** Keep our changes to inherited files isolated and documented in `patches/` or commit messages prefixed `fork:`.
3. **Upstream first.** If a fix is generic (Accessibility, overlay geometry, highlighter bug), send it upstream and cherry-pick the merged commit.
4. **Quarterly review.** Evaluate new Harper releases for security fixes, performance, and grammar rules; update the base only after the full test matrix passes.
5. **Never merge upstream blindly.** The desktop app's UI and config are being rewritten; a merge can regress our branding and settings.
6. **Record the base.** Tag our releases with the upstream base in the release notes (e.g., "Harper base v2.13.0").

## Attribution

- Keep the upstream `LICENSE`.
- Keep `NOTICE` with Automattic's copyright and our modifications.
- About screen: "Built on Harper, an open-source grammar checker by Automattic and contributors."
- Do not use the Harper name or logo as our product identity.
- Do not imply endorsement.

## Divergence log

Maintain a short section here for each upstream sync:

| Date | Upstream ref | Summary | Tests |
| --- | --- | --- | --- |
| 2026-10-19 | v2.13.0 | Initial fork planned; source fetched to `.upstream/harper` | Workspace tests pass; desktop spike pending |
