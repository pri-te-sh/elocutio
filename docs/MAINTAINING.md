# Maintaining Elocutio

This is the repository and release checklist for maintainers. Product scope and
architecture live in [`PLAN.md`](PLAN.md) and [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Repository settings

- **Visibility:** public once the first public milestone is ready. The project is
  meant to be open source; do not keep it private after launch.
- **Description:** `An open-source, bring-your-own-key writing assistant for macOS.`
- **Website:** `https://elocutio.app` once the domain and site are live.
- **Topics:** `macos`, `writing`, `grammar-checker`, `ai`, `llm`, `openrouter`,
  `byok`, `privacy`, `tauri`, `rust`, `open-source`.
- **Social preview:** upload `assets/icons/social-preview.png` (1280×640).
- **License:** GitHub should detect Apache-2.0 from `LICENSE`; keep `NOTICE` visible.

## Security

- Enable **private vulnerability reporting** (Settings → Code security and analysis).
- Enable **Dependabot alerts** and **Dependabot security updates**; the version
  update config is in `.github/dependabot.yml`.
- Enable **secret scanning** and **push protection** if available.
- Do not enable third-party analytics or telemetry in the app.
- Read [`SECURITY.md`](../SECURITY.md) before responding to a report.

## Actions and branch protection

- Enable GitHub Actions for the repository. The CI workflow is
  `.github/workflows/ci.yml`.
- Require the `Rust (fmt, clippy, test)`, `Docs`, and `Web (Prettier,
  Svelte check/build)` jobs before merging to `main`. Do **not** require the
  macOS app workflow — it is manual by design.
- Protect `main`: require a pull request, at least one review, passing checks,
  and conversation resolution. Allow administrators to bypass only in an
  emergency.
- Prefer squash merges; keep the PR title as the commit subject.
- Require signed commits only if every contributor can sign; DCO sign-off is the
  baseline.

## CI minutes (GitHub Free)

GitHub Free includes 2,000 Actions minutes/month, and **macOS runners count at
10×**. Keep routine CI cheap:

- `ci.yml` runs only on Ubuntu: root Rust checks, docs, Prettier, and the
  Svelte frontend check/build.
- `macos-app.yml` is manual, label-triggered (`macos-app`), or tag-triggered.
  Never make it a required check or run it on every push.
- Run `just macos-check` locally before opening a macOS PR.
- Keep `timeout-minutes` on every job; cancel superseded runs with the existing
  concurrency groups.
- Do not add macOS matrix builds or a macOS job to routine CI without a budget
  review.
- Dependabot PRs use the cheap routine CI; label `macos-app` only when the
  dependency needs a full Tauri check.

## Discussions

Enable Discussions with these categories:

- **Announcements** — releases and roadmap updates.
- **Ideas** — feature requests that are not yet issues.
- **Q&A** — setup, providers, and usage.
- **Show and tell** — voice profiles, style guides, workflows.
- **Development** — architecture and contribution discussion.

Link the roadmap issue from Announcements.

## Labels

Create these labels (the defaults `bug`, `enhancement`, `documentation`,
`good first issue`, and `help wanted` already exist):

| Label | Use |
| --- | --- |
| `needs-triage` | New issue, not yet reviewed |
| `area:macos` | Accessibility, overlay, menu bar, packaging |
| `area:voice` | Voice profiles, samples, style cards |
| `area:providers` | OpenRouter and other provider adapters |
| `area:extension` | Browser extension and bridge |
| `area:docs` | Documentation and website |
| `privacy` | Data flow, keys, samples, permissions |
| `blocked` | Waiting on an external decision or upstream |
| `good first issue` | Scoped, with pointers to the relevant files |

## Issue triage

1. Label new issues `needs-triage` within a few days.
2. Ask for a redacted diagnostics export, macOS version, app, and provider/model.
3. Reproduce in at least one Tier A app.
4. Convert accepted work into a scoped issue with acceptance criteria.
5. Mark `good first issue` only when the change is small and the files are known.
6. Close duplicates with a link; do not close "won't fix" without an explanation.

Never ask users to paste API keys, private text, or voice samples.

## Pull requests

- Use the PR template; require a test plan and a docs update when behavior changes.
- Run `just check` before merging.
- Keep [`progress/tracker.json`](../progress/tracker.json) current when a
  milestone gate moves; the live dashboard (`just progress-serve`) is the
  status of record. See [PROGRESS.md](PROGRESS.md).
- For UI changes, require a screenshot or short recording.
- For prompt changes, require golden-set metrics.
- For dependency additions, check license compatibility and maintenance status.
- Keep the `NOTICE` and Harper attribution intact.

## Platform releases

| Platform | Distribution | Gate |
| --- | --- | --- |
| macOS | Developer ID DMG + notarization + Tauri updater | M5 |
| Chrome/Edge | Chrome Web Store / Edge Add-ons | M3 |
| Windows | Signed installer/MSIX + updater | post-v1 |
| iOS | App Store / TestFlight | planned |
| Android | Google Play / direct APK for dev | planned |
| Safari/Firefox | App Store / AMO | later |

Do not announce a platform until its app matrix and store review are complete.

## Releases

The signed/notarized release pipeline is scheduled for M5. When it lands:

1. Update `CHANGELOG.md`.
2. Bump the workspace version and the Tauri app version.
3. Tag `vX.Y.Z` and push the tag.
4. Build a universal macOS app, sign with Developer ID, notarize, staple.
5. Publish the DMG, checksums, and Tauri updater artifacts.
6. Update the Homebrew cask.
7. Announce in Discussions and on the website.

Do not publish an unsigned build as a release. Local dev builds are fine for
testing but will re-prompt for TCC permissions and cannot be verified by users.

## Privacy and security invariants

These must remain true for every release:

- API keys are in the macOS Data Protection Keychain, device-only and non-synchronizable.
- The highlighter subprocess never receives keys.
- The bridge binds to `127.0.0.1` with a pairing token and Origin allowlist.
- The app contacts only configured provider hosts and the update endpoint.
- No telemetry by default.
- Voice samples are encrypted at rest and deletion cascades.
- Secure input fields are never read or written.
- Google Docs is not claimed as supported.

## Naming and trademark

- The product name is **Elocutio** (ADR-020).
- Complete US/EU/UK trademark clearance in classes 9 and 42 before launch.
- `elocutio.app` is planned but not yet registered; do not claim it until it is.
- Do not use Grammarly, Superhuman, or Harper logos or imply endorsement.
- Keep the "built on Harper" attribution and Apache-2.0 notices.

## Maintainer checklist (weekly)

- [ ] Triage new issues and PRs.
- [ ] Review Dependabot PRs; merge safe updates.
- [ ] Check CI and security alerts.
- [ ] Update `CHANGELOG.md` if `main` has user-visible changes.
- [ ] Verify the roadmap and pinned issues are current.
- [ ] Check that no secrets or private samples were committed.
