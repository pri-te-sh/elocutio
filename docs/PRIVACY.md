# Privacy

**Principle:** local-first, provider-optional, and verifiable.

Elocutio is a desktop app, not a service. There is no Elocutio account, no Elocutio inference backend, and no telemetry by default. The only network requests are the ones you configure: your model provider(s) and the update check.

---

## Data flows

This table describes **what the current build does**, not a target state. Every
row names a file you can find in
`~/Library/Application Support/elocutio/`.

| Data | Where it goes | When | User control |
| --- | --- | --- | --- |
| Text being checked | Your chosen provider (or nowhere in local-only mode) | When you trigger analysis or opt into Analyze as I write | Per-app mode, block list, local-only toggle |
| Selected text / context | Your chosen provider | When you use Rewrite Selection | Tone preset, voice toggle, local-only mode |
| API keys | macOS Keychain (`app.elocutio.provider.*`) | On entry | Delete/replace in Settings |
| Usage (tokens, cost, model) | `usage.jsonl` | After each request | View in Settings; delete the file |
| Suggestion events (rule, category, edit size) | `suggestion-events.jsonl` | When you act on a suggestion, if enabled | Off by default; delete in Settings |
| Voice samples (uploads/pastes) | Never stored; analyzed in memory | When you add them | Delete the profile |
| Voice style card | `voice-profile.json`; sent to your provider on voice-aware tasks | When you use "Use my voice" | Preview the exact payload, strength, style-card-only mode |
| Voice feedback (accepted edits) | `voice-feedback.jsonl` | Only when voice learning is on | Disable learning; clear |
| Undo snapshot (before/after field text) | `last-edit-undo.json` | When a suggestion is applied | Deleted after one undo |
| LLM suggestion cache | `llm-cache.json` | After an analysis | Overwritten on the next analysis |
| Crash reports | Nowhere by default | Opt-in only | Toggle |

**Not collected:** analytics, device identifiers, advertising IDs, location,
contacts, calendar, screen contents, or keystrokes.

**Still to build:** a request log, a network log, request preview, provenance
events, a documents editor, and exemplar sending are **not implemented yet**.
They are listed in the plan as M2–M4 work. The `elocutio-store` crate reserves
the SQLite schema and file name, but the current build stores everything in
the plain files listed above.

---

## Voice learning

Voice learning is **off by default**. When you enable it:

- **Samples stay on your Mac during analysis and are then discarded.** v0 stores only the aggregate style card as a local JSON file; it is not encrypted at rest yet. Encryption at rest and a features-only mode remain roadmap work.
- **Only the style card is sent to your provider**, and only when a voice-aware rewrite runs. The current build sends no excerpts: exemplar retrieval is M3 work. Every rewrite is checked against a **copy guard** before it is applied, so a response that repeats your samples back at you is refused.
- **You can preview the exact payload** before the first voice-aware request.
- **You can delete the profile**, which removes the style card, the feedback log, and the learned traits.
- **We never fine-tune a cloud model on your writing.** The profile is a local style card plus retrieval, not training.
- **Optional enrichment:** if you ask Elocutio to analyze humor/empathy nuances with a model, only the selected samples are sent, with a preview and cost estimate. On-device Apple FM/Ollama is preferred; cloud enrichment is off by default.
- **We never infer sensitive attributes** (gender, age, ethnicity, personality) from writing.
- **We never learn from secure fields or password managers**, and never from apps on your block list.
- **AI-generated text you accept unchanged is down-weighted** so the profile does not converge on the model's voice. A model-authored edit carries one tenth the weight of your own.
- **Dismissing a suggestion is a negative signal.** Five dismissals in one category suppress that category's edits.

Voice is intended for your own writing or a brand you are authorized to represent. Uploading someone else's writing to impersonate them is prohibited by the acceptable-use policy.

## Voice retention

| Data | Default | Configurable |
| --- | --- | --- |
| Raw samples | Never stored; only 8-gram hashes are kept | Delete the profile |
| Style card / traits | Until profile deleted | Reset learned values, override any trait |
| Exemplars | Not stored, not sent | — |
| Voice feedback (`voice-feedback.jsonl`) | Your edits' text, truncated to 240 characters; kept 365 days and weighted with a 180-day half-life | Disable learning; clear |
| Voice events (`voice-events.jsonl`) | Rating, tone choice, or override key; no text | Clear |
| Suggestion events (`suggestion-events.jsonl`) | Rule, category, and edit size only; no text | Delete in Settings |
| Undo snapshot (`last-edit-undo.json`) | The field's before/after text, until the next edit | Deleted after one undo |
| Usage metadata (`usage.jsonl`) | timestamp, model, token counts, provider-reported cost; no text | Delete the file from Application Support |

## Cross-device

There is **no cloud sync in v1**. Voice profiles, settings, and style guides can be exported as JSON and imported on another platform. If optional sync is added later, it will be end-to-end encrypted and opt-in; the server will never be able to read samples or profiles.

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

- Stored in the macOS Keychain under `app.elocutio.provider.openrouter` (or `app.elocutio.provider.openai_compatible`), account `api_key`.
- Never written to logs, crash reports, diagnostics, or the config file.
- Passed only to the provider you configured, in the `Authorization` header over HTTPS.
- The highlighter subprocess and the browser extension never receive keys.
- Switching provider kind leaves the previous provider's key in place, so you can switch back. Clear the key in Settings to remove it.

---

## Providers

When you use a cloud provider, your text is processed under that provider's terms and data-retention policy. Elocutio cannot change those policies, but it can help you choose:

- OpenRouter: the default. Set `provider.data_collection: "deny"` and optionally `zdr: true`; note that ZDR does not cover plugins/tools (we do not use them in analysis).
- Direct providers: link to the provider's privacy policy in Settings.
- Local: Ollama, LM Studio, or Apple's on-device model; no network request leaves the Mac.
- Local-only mode: disables all cloud providers; local rules and a local model only.

**Local-only mode** blocks every provider request (including manual analysis, widget AI, and rewrites) and keeps Harper checks on-device.

**Learn from my accepted edits** is off by default too. When enabled, accepted replacements are stored only in a local `voice-feedback.jsonl` file and folded into the style card; disabling it stops new records, and deleting the voice profile removes the learned card. Raw uploaded samples are never stored.

**Analyze as I write** is off by default. When enabled, it waits for a typing pause and analyzes each distinct focused-field text once; turn it off to return to explicitly triggered analysis only.

**Block list.** An app on the block list is never read, never linted, and never
sent to a provider, even if it is also enabled as an integration. Blocking an
app also disables it as an integration, so the two settings cannot contradict
each other.

**Still to build:** a preview of the exact request and a network log of every
host contacted are planned but not implemented. The current build shows the
voice payload that would be sent, and the usage log records the provider and
model for every request.

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

"Clear all data" is not implemented yet. Today, deleting the voice profile removes the style card and accepted-edit feedback; deleting the key removes the Keychain item; usage and the transient caches can be removed from Application Support. A single clear-all action remains roadmap work.

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
