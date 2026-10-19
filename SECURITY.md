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

- API keys are stored in the macOS Data Protection Keychain, device-only, non-synchronizable.
- Keys live only in the main process; the highlighter and browser extension never receive them.
- The local bridge binds to `127.0.0.1`, requires a pairing token, and checks the `Origin`.
- The app makes network requests only to user-configured provider hosts and the update endpoint.
- No telemetry or third-party analytics by default.
- Updates are signed and verified before installation.
- Secure input fields are never read or modified.
- Voice samples are encrypted at rest; deleting a sample/profile cascades to derived data.
- Only the style card and user-reviewed excerpts are sent to providers, with a preview.

## Threat model

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) §14 for the threat table and
mitigations, and [`docs/PRIVACY.md`](docs/PRIVACY.md) for the data-flow model.

## Safe harbor

We will not pursue legal action against researchers who act in good faith, avoid
privacy violations and service disruption, and report findings privately. We will
credit reporters in the advisory unless they prefer to remain anonymous.
