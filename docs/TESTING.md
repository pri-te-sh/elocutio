# Testing

**Goal:** prove that suggestions are correct, edits land on the intended span, the app never steals focus, and no text or key leaks.

---

## 1. Test pyramid

| Layer | What | Tooling | Runs |
| --- | --- | --- | --- |
| Unit | Resolver, chunker, schema, adapters, cost, merge, keychain | `cargo test`, `proptest`, `insta` | Every commit |
| Integration | Analyze → accept → verify; IPC; bridge; updater | `axum` fake provider, temp SQLite | Every PR |
| UI | Onboarding, settings, assistant, card | Playwright (webviews), manual AppKit | Nightly / release |
| Manual matrix | Real apps, displays, permissions | Checklist | Every release |
| Quality eval | Golden set, LLM judge, regression | `tools/eval` | Prompt/model changes |
| Security | Key handling, bridge auth, injection | Targeted tests + review | Release |

---

## 2. Unit tests

### Offset resolver

- Exact match, single occurrence.
- Multiple occurrences: unique anchor required; otherwise drop.
- Normalized match: NFC, curly quotes, dashes, whitespace; index map preserved.
- Fuzzy match: ≥0.85 similarity; below threshold dropped.
- Unicode: emoji (single and ZWJ sequences), flags, combining marks, CJK, RTL.
- Line endings: LF, CRLF, mixed.
- Edge cases: empty `original`, `original == replacement`, span >2×, span >200 chars.
- Property-based: random text + random substring → resolve → re-extract equals original.

### Chunker

- Paragraph and sentence boundaries; no split inside a grapheme cluster.
- 600–1,200 token targets; hard cap.
- One-sentence overlap; stable IDs across edits.
- Dirty-chunk detection via paragraph hashes.

### Schema and merge

- Missing/extra fields; wrong enum; empty replacement.
- Overlapping edits: same/different replacement; severity ordering.
- Local vs LLM overlap: local wins for correctness; LLM wins for judgment.
- Caps: 8/paragraph, 100/document.

### Providers

- Recorded fixtures: success, stream, usage, 401, 402, 429, 400, malformed SSE, mid-stream error.
- Capability checks reject unsupported params before the network call.
- Anthropic: system top-level, `max_tokens`, temperature omission on newer models.
- Gemini: `contents`/`systemInstruction`/`responseSchema`.
- OpenRouter: `require_parameters`, `provider` object, SSE comments, final-chunk usage.
- Retry/backoff honors `Retry-After`; no retry on auth/invalid.
- Cost: per-token strings × 1e6; cache pricing; long-context overrides.

### Keychain

- Store/read/delete; survives restart; not in the legacy login keychain.
- Key absent from logs, diagnostics, and crash payloads.
- Optional biometric gate (manual).

### Voice

- Feature extraction: sentence length, variance, contractions, MATTR, formality, hedging, punctuation, greeting/sign-off on labelled fixtures.
- Confidence: low/medium/high bands; per-trait confidence; language mismatch rejection.
- Profile: decay (180-day half-life), overrides, drift detection, per-context selection, deterministic summary.
- Ingestion: txt/md/docx parsing; quoted-reply stripping; quality gate; dedupe; size limits; corrupt files.
- Copy guard: 8-gram overlap rejection; entity/number invariants; regeneration once.
- Feedback: weights, preference thresholds, AI-text down-weighting, freeze/reset.
- Encryption: sample round-trip with the Keychain key; DB copy without the key reveals no text.
- Deletion: sample delete cascades to exemplars and stats; profile delete cascades to all.

### Merge and ranking

- Resolution quality ordering; severity; document order.
- Duplicate explanation merge.
- Suppression inside code/URL/email spans.

---

## 3. Integration tests

### Fake provider

`tools/fake-provider` (axum) scenarios:

- `ok` — valid JSON, 2 suggestions.
- `slow` — 10 s delay (timeout/cancel).
- `429` — Retry-After 1 s, then success.
- `402` — quota error.
- `malformed` — invalid JSON, then valid on repair.
- `unresolved` — quotes that do not exist in the text.
- `partial-stream` — SSE chunks, then error.
- `empty` — no suggestions.

### End-to-end

1. Seed a document with known errors.
2. Trigger analysis.
3. Assert spans and replacements.
4. Accept one; assert final text and caret.
5. Undo; assert original restored.
6. Assert usage log has tokens/cost and no text.

### IPC

- Version mismatch rejected.
- Oversized message rejected.
- Highlighter crash → restart → focus reacquired.
- Stale response dropped when the field hash changes.

### Voice end-to-end

1. Upload 3 samples in the email context.
2. Assert the style card has at least 5 traits with confidence and a summary.
3. Generate a rewrite with voice on; assert no 8-gram overlap and entities preserved.
4. Rate "sounds like me"; assert the profile updates with low weight.
5. Switch to "style card only"; assert the payload contains no excerpt text.
6. Delete a sample; assert its exemplars and stats contribution are gone.
7. Delete the profile; assert all samples/events are gone.

### Bridge

- Pairing succeeds with the correct code; fails with a wrong code.
- Bad token rejected; bad Origin rejected.
- Reconnect after app restart.
- Rate limit enforced.
- Server not listening when disabled/quit.

---

## 4. Manual matrix

Run before every release; record results in the release checklist.

### Apps

Tier A: TextEdit, Notes, Mail, Messages, Safari, Chrome, Pages, Word, Slack, Obsidian, Gmail (extension), plus one Chromium Electron app.
Tier B: Notion, Linear, Jira, Confluence, VS Code, Excel, PowerPoint, Outlook, Teams.
Unsupported: Google Docs (verify clear message), Terminal, password fields.

### Environment

- macOS current + previous major.
- Light/dark; increased contrast; Reduce Motion; VoiceOver.
- Retina/non-Retina; two monitors with different scaling; Spaces; full-screen; Stage Manager.
- Fresh install; permission revoked mid-session; upgrade; second user.

### Text

Empty, 1 character, 10k characters, emoji, RTL, CJK, code blocks, Markdown, rich text with links, tables, footnotes.

### Network

Offline, slow 3G, captive portal, proxy, VPN, provider outage.

### Security

Secure field, password manager, terminal, screen sharing, clipboard manager, voice sample encryption, style-card-only payload, deleted-sample cascade.

---

## 5. Quality evaluation

### Golden set

200–500 items stratified by:

- error type (grammar, spelling, punctuation, style, clarity, tone, conciseness),
- length (email, paragraph, 1,500-word doc),
- source (native, learner, non-native, domain jargon),
- difficulty (obvious, subtle, no-error control).

### Metrics

| Metric | Target |
| --- | --- |
| Span precision | ≥ 0.85 |
| Span recall | ≥ 0.70 |
| Span F0.5 | ≥ 0.80 |
| Exact acceptance | ≥ 0.60 |
| Resolution rate | ≥ 0.95 |
| Over-edit rate (no-error controls) | ≤ 0.05 |
| Meaning preservation (rewrites) | ≥ 0.98 |
| Schema failure rate | ≤ 0.01 |
| Latency p95 (150 words) | ≤ 4.0 s |
| Cost per 1,000 email reviews | ≤ $0.30 |

Use ERRANT M2 F0.5 and GLEU as secondary signals. LLM-as-judge only for relative comparisons, with randomized order and human calibration.

### Voice metrics

| Metric | Target |
| --- | --- |
| "Sounds like me" after 3 samples | ≥ 4.0 / 5 |
| Sentence-length distribution similarity to held-out | Within 15% |
| Contraction rate similarity | Within 0.1 / 100 words |
| Formality similarity | Within 0.4 / 5 |
| 8-gram overlap with samples | 0 |
| Meaning preservation | ≥ 98% |
| Edit distance after generation | ≤ 20% of words |
| Feedback-loop drift (50 unedited accepts) | No significant trait shift |

### Regression gate

Fail CI if:

- F0.5 drops >2 points,
- resolution rate drops >5 points,
- cost rises >20%,
- over-edit rate rises >2 points.

---

## 6. Performance tests

- `cargo bench`: resolver, chunker, merge.
- Signposts: capture → IPC → provider → render.
- Instruments: Time Profiler, Allocations, Energy Log.
- Budgets: local lint <10 ms; IPC <5 ms; widget <80 ms; idle CPU <0.5%; memory <220 MB.
- Soak test: 8 hours idle + periodic typing; check leaks and CPU.

---

## 7. Security tests

- Grep CI for accidental key/text logging.
- Assert `Authorization` headers are redacted in diagnostics.
- Assert the highlighter process has no provider config/keys.
- Bridge: token, Origin, loopback, rate limit, close-on-quit.
- Update: signature verification with a tampered payload.
- Injection: secure field guard; `IsSecureEventInputEnabled`; clipboard restore.
- Dependency audit: `cargo audit`, `pnpm audit`, license check.

---

## 8. Platform expansion testing

Each new platform gets its own matrix and must not inherit a "supported" claim from macOS.

| Platform | Minimum tests before announcement |
| --- | --- |
| **Chrome/Edge** | Gmail, Notion, Slack web, Linear, textarea; pairing; unpaired rejection; no keys in extension |
| **Windows** | Notepad, Word, Outlook, Chrome, Slack; UIA read/write; clipboard fallback; signed installer; SmartScreen |
| **iOS** | Keyboard install + full-access prompt, share extension, in-app editor, voice profile import/export, App Store review |
| **Android** | Keyboard enable/switch, 5 apps with AccessibilityService, overlay permission, Play policy review, battery/background limits |
| **Safari/Firefox** | Extension load, contenteditable adapter, native messaging/WS, store review |

Shared fixtures and the fake provider should be reused; do not duplicate the analysis tests per platform.

## 9. Release checklist

- [ ] Unit + integration + frontend tests pass.
- [ ] Golden set meets thresholds; no regression.
- [ ] Manual matrix completed on current and previous macOS.
- [ ] Accessibility: VoiceOver, keyboard-only, contrast.
- [ ] Signing, notarization, stapling, `spctl` pass.
- [ ] Clean-Mac install + onboarding + update test.
- [ ] Network log shows only provider + update hosts.
- [ ] Voice: off by default; samples encrypted; style-card-only sends no excerpts; delete cascades; copy guard passes.
- [ ] "Sounds like me" and held-out trait similarity meet targets on the voice regression set.
- [ ] No keys/text in logs, crash reports, or diagnostics.
- [ ] Privacy policy and support matrix updated.
- [ ] Rollback appcast entry ready.
- [ ] Changelog and release notes published.
