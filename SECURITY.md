# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

**Preferred:** use GitHub's private vulnerability reporting on the **Security** tab of
<https://github.com/pri-te-sh/elocutio> ("Report a vulnerability"). This creates a
private advisory visible only to the maintainers.

If private reporting is unavailable, email `security@elocutio.app` with:

- a description of the issue,
- steps to reproduce,
- affected version/commit,
- impact assessment,
- any suggested fix.

We aim to acknowledge within **72 hours** and provide a remediation plan within
**7 days**. Please give us reasonable time before public disclosure.

## Scope

In scope:

- API key storage and exposure
- Voice-sample storage, encryption, deletion, and excerpt leakage
- Local bridge authentication and origin checks
- Prompt injection leading to data exfiltration
- Update signature verification
- Text injection that corrupts user data
- Permission bypass (secure fields, blocked apps, excluded sites)
- Dependency vulnerabilities in shipped code

Out of scope:

- Issues requiring a jailbroken or already-compromised macOS
- Social engineering
- Provider-side outages, pricing, or policy decisions
- Vulnerabilities in third-party models or providers
- "The model hallucinated an edit" (quality issue, not a security issue)
- Trademark or naming disputes (contact the maintainers directly)

## Design commitments

These are the guarantees the project holds itself to. The **Status** column
says which ones the current build actually provides, so this file never claims
a protection the code does not have yet.

| Commitment | Status |
| --- | --- |
| API keys are stored in the macOS Keychain, never in the config file or logs | **Implemented** |
| Keys live only in the main process; the highlighter and browser extension never receive them | **Implemented** |
| The app makes network requests only to user-configured provider hosts | **Implemented** |
| No telemetry or third-party analytics by default | **Implemented** |
| Secure input fields are never read or modified | **Implemented** |
| Apps on the block list are never read or sent to a provider | **Implemented** |
| The usage log records metadata only, never your text | **Implemented** |
| The local bridge binds to `127.0.0.1`, requires a pairing token, and checks the `Origin` | **Planned (M3)** — no bridge server exists yet |
| Updates are signed and verified before installation | **Planned (M5)** — the updater is removed for now, so there is no unsigned-update path |
| Voice samples are encrypted at rest | **Planned** — the current build stores only the aggregate style card, in a plain local file |
| Deleting a sample/profile cascades to derived data | **Implemented for the profile**; per-sample deletion arrives with M3 uploads |
| Only the style card is sent to providers, with a preview | **Implemented** — no excerpts are sent yet |

The inherited Harper code has **not** yet had the security review that
`docs/PLAN.md` lists as a fork condition. See
[`docs/M0-VERIFICATION.md`](docs/M0-VERIFICATION.md).

## Threat model

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) §14 for the threat table and
mitigations, and [`docs/PRIVACY.md`](docs/PRIVACY.md) for the data-flow model.

## Safe harbor

We will not pursue legal action against researchers who act in good faith, avoid
privacy violations and service disruption, and report findings privately. We will
credit reporters in the advisory unless they prefer to remain anonymous.
