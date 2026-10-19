# Privacy

**Principle:** local-first, provider-optional, and verifiable.

Elocutio is a desktop app, not a service. There is no Elocutio account, no Elocutio inference backend, and no telemetry by default. The only network requests are the ones you configure: your model provider(s) and the update check.

---

## Data flows

| Data | Where it goes | When | User control |
| --- | --- | --- | --- |
| Text being checked | Your chosen provider (or nowhere in local-only mode) | When you trigger analysis or enable live mode | Per-app mode, block list, local-only toggle, request preview |
| Selected text / context | Your chosen provider | When you use rewrite, compose, reply, summarize, chat | Context chips; remove before sending |
| API keys | macOS Data Protection Keychain | On entry | Delete/replace; optional biometric gate |
| Usage (tokens, cost, model) | Local SQLite | After each request | View/export/clear |
| Request log (host, model, status, cost) | Local SQLite | After each request | View/export/clear; no text |
| Accepted/dismissed suggestions | Local SQLite | When you act on a suggestion | Off by default; clear |
| Provenance events (hashes, not text) | Local SQLite | Opt-in | Off by default; export/clear |
| Documents (Docs editor) | Local SQLite | When you create one | Delete |
| Voice samples (uploads/pastes) | Local SQLite, encrypted at rest | When you add them | Delete sample/profile; "features only" mode |
| Voice style card + exemplars | Local SQLite; sent to your provider on voice-aware tasks | When you use "Use my voice" | Preview, strength, style-card-only mode |
| Voice feedback (accept/dismiss/rate) | Local SQLite | Only when voice learning is on | Disable learning; clear |
| Crash reports | Nowhere by default | Opt-in only | Toggle |

**Not collected:** analytics, device identifiers, advertising IDs, location, contacts, calendar, screen contents, or keystrokes.

---

## Voice learning

Voice learning is **off by default**. When you enable it:

- **Samples stay on your Mac**, encrypted at rest with AES-256-GCM; the data key is wrapped by the macOS Keychain. A "features only" mode stores no raw text.
- **Only the style card and 2–4 user-reviewable excerpts** are sent to your provider, and only when a voice-aware rewrite/compose/reply runs. "Style card only" mode sends no excerpts.
- **You can preview the exact payload** before the first voice-aware request.
- **You can delete any sample**, which removes its exemplars and its contribution to the profile.
- **You can delete the profile**, which removes all samples, exemplars, feedback, and derived traits.
- **We never fine-tune a cloud model on your writing.** The profile is a local style card plus retrieval, not training.
- **Optional enrichment:** if you ask Elocutio to analyze humor/empathy nuances with a model, only the selected samples are sent, with a preview and cost estimate. On-device Apple FM/Ollama is preferred; cloud enrichment is off by default.
- **We never infer sensitive attributes** (gender, age, ethnicity, personality) from writing.
- **We never learn from secure fields, terminals, password managers, or apps/sites you exclude.**
- **AI-generated text you accept unchanged is down-weighted** so the profile does not converge on the model's voice.

Voice is intended for your own writing or a brand you are authorized to represent. Uploading someone else's writing to impersonate them is prohibited by the acceptable-use policy.

## Voice retention

| Data | Default | Configurable |
| --- | --- | --- |
| Raw samples | Encrypted, until deleted | Features-only mode; delete all |
| Style card / traits | Until profile deleted | Reset learned values |
| Exemplars | Until sample/profile deleted | Exclude sensitive samples |
| Feedback events | 180-day half-life weighting; rows until cleared | Clear |

---

## Permissions

| Permission | Why | What it allows | What it does not allow |
| --- | --- | --- | --- |
| Accessibility | Read the focused text field and replace text | Reading the focused element's value/selection; setting the value or pasting | Reading other windows, screen contents, or keystrokes |
| Screen Recording | Optional OCR fallback | Capturing the screen region for text recognition | Always-on capture (only on explicit action) |
| Input Monitoring | Not requested | — | — |
| Automation | Not requested | — | — |

Elocutio never reads password fields, secure input, or terminals. It disables itself when `IsSecureEventInputEnabled()` is true or the focused element is a secure text field.

---

## Keys

- Stored with the macOS Data Protection Keychain (`kSecUseDataProtectionKeychain`), `WhenUnlockedThisDeviceOnly`, non-synchronizable.
- Never written to logs, crash reports, diagnostics, or the database.
- Passed only to the provider you configured, in the `Authorization` header over HTTPS.
- The highlighter subprocess and the browser extension never receive keys.
- Deleting a provider deletes its keychain item.

---

## Providers

When you use a cloud provider, your text is processed under that provider's terms and data-retention policy. Elocutio cannot change those policies, but it can help you choose:

- OpenRouter: set `provider.data_collection: "deny"` and optionally `zdr: true` by default; note that ZDR does not cover plugins/tools (we do not use them in analysis).
- Direct providers: link to the provider's privacy policy in Settings.
- Local: Ollama, LM Studio, or Apple's on-device model; no network request leaves the Mac.
- Local-only mode: disables all cloud providers; local rules and a local model only.

The app shows a **preview of the exact request** (redacted key) before sending, and a **network log** of every host contacted.

---

## Retention

| Data | Default | Configurable |
| --- | --- | --- |
| Analyzed text | Not stored | — |
| Suggestions cache | Memory only | Optional disk cache, TTL 24 h |
| Usage/cost | 90 days | Yes |
| Request log | 7 days | Yes |
| Accepted/dismissed events | Off | Yes |
| Provenance | Off | Yes |
| Documents | Until deleted | Yes |

"Clear all data" removes usage, logs, cache, events, provenance, documents, and voice samples/profiles. Keys are deleted separately with a confirmation.

---

## Verifiable claims

We say **"no first-party inference backend"**, not "no network calls." The app does contact your provider and the update server. To verify:

1. Read the source (Apache-2.0).
2. Open Settings → Privacy → Network log to see every host, model, token count, and cost.
3. Use a network monitor (Little Snitch, `nettop`) while the app runs.
4. Build from source and compare the bundle.
5. Voice: open Settings → Voice → "What will be sent" to see the exact style card and exemplars; enable "style card only" to send no excerpts.

There is no hidden analytics endpoint, no crash SDK, and no remote configuration fetch.

---

## Children and sensitive data

Elocutio is a general writing tool, not a service that collects personal data. Do not use it to process data you are not permitted to send to your chosen provider. For regulated data, use local-only mode or a provider with an appropriate agreement.

---

## Changes

Material changes to this policy will be documented in the repository and, for released versions, in the app's release notes. The source of truth is this file.
