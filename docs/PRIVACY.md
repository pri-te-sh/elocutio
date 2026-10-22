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
| Text being checked | Your chosen provider (or nowhere in local-only mode / Apple Foundation Models / Ollama Local) | When you trigger analysis or opt into Analyze as I write | Per-app mode, block list, local-only toggle |
| Selected text / context | Your chosen provider | When you use Rewrite Selection | Tone preset, voice toggle, local-only mode |
| API keys | macOS Keychain (`app.elocutio.provider.*`) + AES-256-CBC encrypted `encrypted_credentials` table in `elocutio.sqlite3` | On entry | Delete/replace in Settings |
| Usage & token ledger (tokens, cost, model, latency) | `usage.jsonl` + `ai_token_usage` table in `elocutio.sqlite3` (**no user text stored**) | After each request | View in Privacy & AI or Analytics; reset anytime |
| Personal dictionary & rule overrides | `user_dictionary.txt` + `dictionary_words` & `editorial_rules` tables in `elocutio.sqlite3` | When you add words or toggle rules | Search, edit, bulk-import, or delete in Lexicon |
| Suggestion & edit events (rule, category, snippet) | `suggestion-events.jsonl` + `edit_events` table in `elocutio.sqlite3` | When you act on a suggestion | Clear history in Settings |
| Voice samples (uploads/pastes) | Never stored raw; analyzed in memory | When you add them | Delete the profile |
| Voice style card & DNA traits | `voice-profile.json` + `voice_profile` table in `elocutio.sqlite3`; sent to your provider on voice-aware tasks | When you use "Use my voice" | Preview the exact payload, strength, style-card-only mode |
| Voice feedback (accepted edits) | `voice-feedback.jsonl` | Only when voice learning is on | Disable learning; clear |
| Undo snapshot (before/after field text) | `last-edit-undo.json` | When a suggestion is applied | Deleted after one undo |
| LLM suggestion cache | `llm-cache.json` | After an analysis | Overwritten on the next analysis |
| Crash reports | Nowhere by default | Opt-in only | Toggle |

**Not collected:** analytics, device identifiers, advertising IDs, location,
contacts, calendar, screen contents, or keystrokes.

**Local SQLite database (`elocutio.sqlite3`):** All personal dictionary entries, rule overrides, learned Voice DNA traits, edit telemetry (`edit_events`), per-request token & USD cost logs (`ai_token_usage`), and AES-256-CBC encrypted API credentials (`encrypted_credentials`) are stored locally in `~/Library/Application Support/elocutio/elocutio.sqlite3`. API keys encrypted in SQLite use a per-installation random salt (`elocutio_keystore.salt`) and 10,000-iteration PBKDF2-HMAC-SHA256 key derivation alongside macOS Keychain storage.

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
| Network log (`network-log.jsonl`) | Host, provider, model, token counts, cost, and whether the request succeeded; no text | Delete the log in Settings |
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

**The network log** records every host the app has contacted, with the model,
the token counts, and the cost. It is the proof behind "no telemetry or hidden
network calls": open Settings → Network log and you can see every request that
left the Mac, and no others. The log holds metadata only — never the text
that was sent.

**The request preview** shows the request the app is about to make: the host,
the model, the estimated cost, and the text exactly as it will be sent. The
API key is redacted, because a preview that printed the key would be a key on
screen. Press **Preview the request** in Settings to see it.

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
2. Open Settings → Network log to see every host, model, token count, and cost.
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
