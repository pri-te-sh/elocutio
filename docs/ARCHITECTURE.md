# Architecture — Elocutio for macOS

**Status:** draft for implementation
**Base:** Harper Desktop v2.13.0 (Apache-2.0), rebranded and extended
**App type:** non-sandboxed, Developer ID-signed, notarized menu-bar agent
**Stack:** Rust 2024, Tauri 2, Svelte 5 + TypeScript, harper-core, SQLite, Keychain

This document is the engineering contract. Product scope and milestones live in [`PLAN.md`](PLAN.md); the evidence for the platform choices lives in [`../research/macos-integration.md`](../research/macos-integration.md) and [`../research/llm-layer.md`](../research/llm-layer.md).

---

## 1. Goals and constraints

### Goals

1. Read the focused text field in other apps and show suggestions without stealing focus.
2. Run local correctness checks in <10 ms and LLM judgment after a debounce or explicit action.
3. Accept an edit reliably, with read-back verification and sane undo.
4. Keep API keys in the main process; never expose them to the highlighter or the browser.
5. Support OpenRouter by default and add providers without changing the analysis pipeline.
6. Learn the user's voice from uploads and feedback, locally and explainably, and apply it across providers.
7. Be honest about what is not supported (Google Docs, secure fields, terminals).
8. Keep the core (analysis, providers, voice, storage, protocol) platform-agnostic so Windows, browser, iOS, and Android can reuse it behind thin platform shells.

### Hard constraints

| Constraint | Consequence |
| --- | --- |
| App Sandbox cannot read other apps via Accessibility | Non-sandboxed Developer ID app; no Mac App Store |
| Chromium ignores AX `AXValue`/`AXSelectedText` writes | Pasteboard + ⌘V is the primary write path outside AppKit |
| `kAXSelectedTextAttribute` is not writable | Never set it; splice `AXValue` or synthesize input |
| `CGEventKeyboardSetUnicodeString` may be ignored by frameworks | Not a primary write mechanism |
| Focus stealing breaks the paste | All overlays non-activating, `canBecomeKey = false` |
| TCC identity is tied to the code signature | Stable dev certificate and fixed bundle ID from day one |
| Google Docs is canvas-rendered | Declared unsupported in v1 |
| Screen Recording re-prompts | OCR is opt-in, never default |
| Harper is English-only | Local rules English; LLM handles other languages |

---

## 2. Why this architecture

The product is three systems:

1. **A read path** — Accessibility for native/web text.
2. **A write path** — a ladder of AX fast path and pasteboard + ⌘V, with verification.
3. **A focus-preserving overlay** — non-activating panels and click-through underline windows.

Harper Desktop already implements 1, 3, and much of 2 on macOS, plus a fast local grammar engine and a tray/updater shell, under Apache-2.0. Rebuilding them is the slowest path to a product and the least differentiated work. We therefore fork the shell and spend our effort on:

- the provider/BYOK layer,
- the LLM judgment pipeline and its validation,
- the suggestion UX,
- the browser extension,
- privacy, cost, and personalization.

The trust boundary is deliberate: **the main process owns secrets and network; the highlighter owns text and injection.** They communicate over a versioned local IPC protocol. Voice profiles are built and stored in the main process (`elocutio-voice`); only the style card and exemplars are sent to a provider.

---

## 3. Process model

```text
┌────────────────────────────── Tauri main process ──────────────────────────────┐
│                                                                                │
│  Tray / menu / lifecycle / autostart / updater                                 │
│                                                                                │
│  ┌──────────────────┐    ┌─────────────────────┐    ┌───────────────────────┐  │
│  │ ProviderManager  │    │ AnalysisCoordinator │    │ BridgeServer          │  │
│  │ - Keychain       │◄──►│ - chunk             │◄──►│ - WebSocket 127.0.0.1 │  │
│  │ - HTTP/SSE       │    │ - prompt + voice    │    │ - pairing token       │  │
│  │ - capabilities   │    │ - validate          │    │ - Origin allowlist    │  │
│  │ - retries/cost   │    │ - resolve offsets   │    └───────────────────────┘  │
│  └──────────────────┘    │ - dedupe/merge      │                               │
│                          │ - cache             │                               │
│  ┌──────────────────┐    └──────────┬──────────┘                               │
│  │ Store (SQLite)   │               │                                          │
│  │ usage, config,   │               │  IPC: JSON lines over stdin/stdout        │
│  │ voice, style,    │               │  (versioned, length-prefixed)             │
│  │ provenance       │               │                                          │
│  └──────────────────┘               │                                          │
└─────────────────────────────────────┼──────────────────────────────────────────┘
                                      │
┌─────────────────────────────────────▼──────────────────────────────────────────┐
│ Highlighter subprocess (Rust + egui/winit + objc2)                            │
│                                                                                │
│  ┌────────────────────┐  ┌────────────────────┐  ┌──────────────────────────┐  │
│  │ AX Broker          │  │ LocalEngine        │  │ Overlay                  │  │
│  │ focused element    │  │ harper-core        │  │ underlines (level 1500)  │  │
│  │ role/secure gate   │  │ <10 ms             │  │ widget (non-activating)  │  │
│  │ text/selection     │  │ dictionary/dialect │  │ cards (non-activating)   │  │
│  │ range bounds       │  └────────────────────┘  │ hit-testing              │  │
│  │ app catalog        │                          └──────────────────────────┘  │
│  │ window stability   │  ┌──────────────────────────────────────────────────┐  │
│  └────────────────────┘  │ Injector: re-read → AX fast path → pasteboard+⌘V │  │
│                          │ → read-back verify → report                      │  │
│                          └──────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────────────┘
```

### Why two processes

| Concern | Main | Highlighter |
| --- | --- | --- |
| API keys | ✅ | ❌ |
| Network | ✅ | ❌ (except IPC to main) |
| Accessibility | ❌ | ✅ |
| User text | receives chunks for analysis | owns the live field |
| Overlay rendering | ❌ | ✅ |
| Injection | ❌ | ✅ |
| SQLite | ✅ | ❌ (asks main) |
| Crash blast radius | settings/analysis lost | overlay restarts without losing keys |

The highlighter can crash and be restarted without exposing secrets. The main process can be audited for network egress independently.

### Alternative considered

Running the LLM in the highlighter would remove IPC latency (~2–5 ms) but put keys and network in the process that touches arbitrary apps' text. Rejected on security grounds. The IPC cost is negligible relative to model latency.

---

## 4. Repository and fork strategy

### 4.1 Upstream base

- Fork `Automattic/harper` at tag **v2.13.0** (16 Oct 2026).
- Keep `upstream` remote. `UPSTREAM.md` records the base commit, local patches, and sync procedure.
- Vendor `harper-core` and its path dependencies in the same workspace so the build is reproducible.
- If the monorepo proves too large, extract `apps/macos` + the required `harper-*` crates into a smaller workspace and pin `harper-core` by git tag. Decide in M0 after measuring clean build time.

### 4.2 Rename map

| Harper | Elocutio |
| --- | --- |
| `harper-desktop` | `apps/macos` (product name: Elocutio) |
| productName `Harper` | `Elocutio` |
| identifier `com.elijahpotter.harper-desktop` | `app.elocutio.desktop` |
| tray title/menu | Elocutio |
| config dir `~/Library/Application Support/...` | `~/Library/Application Support/Elocutio` |
| updater endpoint | `https://updates.elocutio.app/...` |
| updater pubkey | our minisign key |
| icons, screenshots, links | our assets |
| "Harper" strings, docs URLs, Weirpacks branding | Elocutio equivalents |

Keep the upstream `LICENSE` and add:

```
NOTICE

Elocutio is derived from Harper (https://github.com/Automattic/harper),
Copyright (c) Automattic, licensed under the Apache License 2.0.
Modifications Copyright (c) Elocutio contributors, licensed under Apache-2.0.
Elocutio is not affiliated with or endorsed by Automattic.
```

### 4.3 Workspace

```
elocutio/
├── Cargo.toml                 # workspace
├── LICENSE / NOTICE / UPSTREAM.md
├── harper-core/               # vendored
├── harper-*/                  # vendored path deps
├── apps/
│   ├── macos/                  # v1: forked Harper shell
│   │   ├── src-tauri/
│   │   │   ├── Cargo.toml
│   │   │   ├── tauri.conf.json
│   │   │   └── src/
│   │   │       ├── main.rs
│   │   │       ├── tray.rs
│   │   │       ├── config/
│   │   │       ├── command/    # Tauri commands
│   │   │       ├── llm/        # coordinator, IPC, events
│   │   │       ├── store/
│   │   │       ├── bridge/
│   │   │       └── (inherited) mac_broker/, highlighter/, ...
│   │   └── src/                # Svelte UI
│   ├── windows/                # post-v1: Tauri + UIA
│   ├── ios/                    # planned: SwiftUI + keyboard/share
│   └── android/                # planned: Compose + IME/accessibility
├── crates/
│   ├── elocutio-core/
│   ├── elocutio-analysis/
│   ├── elocutio-llm/
│   ├── elocutio-voice/         # samples, traits, style card, feedback
│   ├── elocutio-store/
│   ├── elocutio-bridge/
│   └── elocutio-platform/      # shared capture/injection/overlay traits
├── extensions/
│   ├── chrome/
│   ├── safari/                 # later
│   └── firefox/                # later
├── tools/fake-provider/
└── scripts/
```

### 4.4 Cross-platform architecture

The product is **one core + N thin shells**. macOS is the first shell, not the architecture.

| Layer | Shared across platforms | Platform-specific |
| --- | --- | --- |
| Domain / suggestions | `elocutio-core` | — |
| Chunking / validation / span resolution | `elocutio-analysis` | — |
| Providers, streaming, retries, cost | `elocutio-llm` | Network/key storage in the shell |
| Voice profiles / style cards | `elocutio-voice` | Sample ingestion UI, encryption key storage |
| SQLite schema / repositories | `elocutio-store` | Database location, file protection |
| Browser protocol | `elocutio-bridge` | Transport (WS/native messaging) |
| Capture / injection / overlay | `elocutio-platform` traits | macOS AX, Windows UIA, Android accessibility, iOS keyboard |
| UI tokens | `assets/brand/tokens.json` | Native controls and platform chrome |
| Distribution | — | DMG/notarization, MSIX, App Store, Play Store, extension stores |

**Platform trait sketch:**

```rust
pub trait TextCapture {
    fn focused_field(&self) -> Result<Option<CapturedField>, PlatformError>;
}

pub trait TextInjector {
    fn replace(&self, field: &CapturedField, span: TextSpan, replacement: &str) -> Result<InjectReport, PlatformError>;
}

pub trait OverlayHost {
    fn show_suggestions(&self, anchor: ScreenRect, suggestions: &[Suggestion]) -> Result<(), PlatformError>;
    fn hide(&self);
}
```

**Hard platform differences:**

- macOS: Accessibility API + pasteboard/⌘V; non-sandboxed; no Mac App Store.
- Windows: UI Automation + clipboard/Ctrl+V; signed installer; SmartScreen reputation.
- iOS: no cross-app accessibility; keyboard extension and share sheet only; App Store review.
- Android: AccessibilityService is permission-heavy and policy-sensitive; keyboard is the reliable fallback.
- Browser: DOM only; Google Docs canvas is unsupported.

**Rules:** no provider keys outside the desktop main process; no platform code in shared crates; voice profiles are portable JSON; never claim a platform until it has a tested app matrix.

---

## 5. Core data types

### 5.1 Suggestions

A suggestion is the unit of UI, persistence, and application.

```rust
pub enum SuggestionSource {
    Local { rule: String },
    Llm { provider: String, model: String, request_id: String },
}

pub enum Category {
    Grammar, Spelling, Punctuation, Capitalization,
    Clarity, Conciseness, WordChoice, Consistency,
    Tone, Style, Formatting,
}

pub enum Severity { Low, Medium, High }

pub struct Suggestion {
    pub id: String,
    pub span: TextSpan,             // UTF-16 + grapheme offsets
    pub original: String,
    pub replacement: String,        // "" = delete
    pub category: Category,
    pub severity: Severity,
    pub explanation: String,
    pub source: SuggestionSource,
    pub resolution: Resolution,     // Exact | Normalized | Fuzzy
    pub actions: Vec<SuggestionAction>,
}

pub struct TextSpan {
    pub utf16_start: usize,
    pub utf16_end: usize,
    pub grapheme_start: usize,
    pub grapheme_end: usize,
}

pub enum SuggestionAction {
    Accept, Dismiss, Explain, AddToDictionary,
    IgnoreRule(String), DisableCategory(Category),
}
```

Why both offset systems:

- Harper and Rust strings use UTF-8/char indices.
- AppKit/TextKit and AX ranges use **UTF-16** (`NSRange`).
- Swift `String.Index` is grapheme-cluster based.
- Emoji, flags, combining marks, and CJK break naive arithmetic.

Convert once at the boundary; never recompute ad hoc.

### 5.2 Analysis request/response

```rust
pub struct AnalysisRequest {
    pub id: RequestId,
    pub app: AppContext,           // bundle id, pid, window title hash (optional)
    pub text: String,
    pub paragraphs: Vec<Paragraph>, // id + global UTF-16 range
    pub selection: Option<TextSpan>,
    pub options: AnalysisOptions,  // categories, dialect, tone, style rules, locale
    pub mode: AnalysisMode,        // Selection | Paragraph | FullDocument
}

pub struct AnalysisResult {
    pub request_id: RequestId,
    pub suggestions: Vec<Suggestion>,
    pub tone: Option<ToneResult>,
    pub usage: Option<Usage>,
    pub warnings: Vec<Warning>,    // unresolved spans, truncation, etc.
}
```

### 5.3 Tone

```rust
pub struct ToneResult {
    pub label: ToneLabel,          // neutral, confident, friendly, formal, ...
    pub confidence: f32,           // display only; not a model self-report
    pub emoji: String,             // derived locally from label
}
```

---

## 6. Provider layer (`elocutio-llm`)

### 6.1 Canonical types

Do not model the internal API on OpenAI's schema. Use a canonical request and let adapters compile it.

```rust
pub struct ChatRequest {
    pub model: ModelRef,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,      // intent; adapter may drop
    pub top_p: Option<f32>,
    pub stop: Vec<String>,
    pub stream: bool,
    pub response_format: ResponseFormat,
    pub reasoning: Reasoning,
    pub routing: Option<RoutingPrefs>, // OpenRouter-specific
    pub session_id: Option<String>,
}

pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema { name: String, schema: serde_json::Value, strict: bool },
}
```

```rust
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub provider: Option<String>,
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
    pub usage: Usage,
}

pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}
```

```rust
pub enum StreamEvent {
    Start { model: String, provider: Option<String> },
    TextDelta(String),
    ReasoningDelta(String),
    Usage(Usage),
    Done { finish_reason: FinishReason },
    Error(ProviderError),
}
```

### 6.2 Provider trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self, model: &str) -> ProviderCapabilities;
    async fn validate(&self) -> Result<ProviderStatus, ProviderError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError>;
    fn stream(&self, req: ChatRequest) -> BoxStream<'static, Result<StreamEvent, ProviderError>>;
    fn estimate_cost(&self, usage: &Usage, model: &ModelInfo) -> Option<f64>;
}
```

### 6.3 Capability descriptor

```rust
pub struct ProviderCapabilities {
    pub system_prompt: SystemPromptStyle,   // Message | TopLevel | Instruction
    pub structured_output: StructuredOutput,// JsonSchema | JsonObject | Tool | None
    pub strict_schema: bool,
    pub tools: bool,
    pub streaming: bool,
    pub reasoning: ReasoningStyle,          // Effort | Budget | Both | None
    pub reasoning_mandatory: bool,
    pub temperature: Option<(f32, f32)>,    // None = do not send
    pub max_output_field: MaxOutputField,
    pub requires_max_tokens: bool,
    pub usage_in_stream: bool,
    pub max_context_tokens: u64,
    pub max_output_tokens: u64,
}
```

The adapter checks capabilities **before** the network call and returns `Unsupported` rather than letting the provider 400.

### 6.4 Adapters

| Adapter | Covers | Notes |
| --- | --- | --- |
| `OpenAiCompatible` | OpenRouter, OpenAI, Groq, Mistral, DeepSeek, xAI, Together, Fireworks, Ollama, LM Studio, custom | Base URL + headers + capability table per provider |
| `Anthropic` | Claude direct | System prompt top-level; `max_tokens` required; temperature/top_p deprecated on newer models (send only when allowed); `output_config.format` for structured output |
| `Gemini` | Google AI Studio / Vertex | `contents`/`parts`/`systemInstruction`/`generationConfig`/`responseSchema` |
| `AppleFoundation` | macOS 26+ | Sidecar or Swift helper; 4,096-token context; availability check; no network |

OpenRouter is the default `OpenAiCompatible` instance with extra headers and routing:

```json
{
  "model": "anthropic/claude-haiku-5.5",
  "messages": [...],
  "response_format": {
    "type": "json_schema",
    "json_schema": { "name": "edits", "strict": true, "schema": { ... } }
  },
  "provider": {
    "require_parameters": true,
    "data_collection": "deny",
    "zdr": true,
    "allow_fallbacks": true
  },
  "reasoning": { "effort": "none" },
  "temperature": 0
}
```

`require_parameters: true` is mandatory whenever structured output is requested; without it, OpenRouter may route to an endpoint that ignores `response_format`.

### 6.5 Streaming

- Parse SSE manually or with `eventsource-stream`; skip lines starting with `:` (OpenRouter sends `: OPENROUTER PROCESSING`).
- OpenRouter can return HTTP 200 with a top-level `error` and `finish_reason: "error"` mid-stream; surface it as `ProviderError`.
- The final OpenRouter chunk may include a non-empty `choices` array with usage; do not assume `choices: []`.
- Abort: drop the stream and cancel the `reqwest` future; emit `Error(Cancelled)` at most once.
- Normalize all provider streams to `StreamEvent`; the UI never parses provider JSON.

### 6.6 Errors and retries

```rust
pub enum ErrorKind {
    Auth, RateLimit, Quota, InvalidRequest, ContextLength,
    ContentFilter, Overloaded, Timeout, Network, Server,
    Unsupported, Cancelled, Unknown,
}
```

Retry policy:

- Retry only `408`, `429`, `5xx`, `529`, network errors.
- Honor `Retry-After` (seconds or HTTP date).
- Jittered exponential backoff: 500 ms → 1 s → 2 s → 4 s → 8 s cap; max 3 attempts.
- Per-provider circuit breaker: open after 5 consecutive failures, half-open after 30 s.
- Timeouts: 10 s connect, 30 s first byte, 120 s total.
- Never retry a 401/402/403 or an invalid request.

### 6.7 Cost

- Prefer provider-reported `usage.cost` (OpenRouter).
- Otherwise compute from the model catalog (`pricing.prompt` / `pricing.completion` per token × 1e6).
- Include cache-read pricing where present.
- Store `cost_usd` per request; aggregate per day/model/task.
- Show a pre-flight estimate for documents over 5,000 characters.

### 6.8 Key management

- macOS Keychain, Data Protection keychain:
  - `kSecClassGenericPassword`
  - `kSecUseDataProtectionKeychain: true`
  - `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`
  - `kSecAttrSynchronizable: false`
  - optional `SecAccessControl` with `.userPresence`
- Service: `app.elocutio.provider.<provider-id>`, account `api-key`.
- Never log the key; redact `Authorization` headers in diagnostics.
- Validate with a free probe: OpenRouter `GET /api/v1/key`; other providers `GET /models`.
- A 1-token completion is an explicit "Test connection" action, not automatic.

---

## 7. Analysis pipeline (`elocutio-analysis`)

### 7.1 Flow

```text
Text (UTF-8)
  │
  ├─► LocalEngine (harper-core) ──► local suggestions (immediate)
  │
  └─► Chunker ──► PromptBuilder ──► Provider ──► JSON
                                                │
                                                ▼
                                      Schema validation
                                                │
                                                ▼
                                      Offset resolver
                                                │
                                                ▼
                                      Dedupe + merge
                                                │
                                                ▼
                                      Rank + cap
                                                │
                                                ▼
                                      AnalysisResult
```

### 7.2 Chunking

- Split on blank lines (paragraphs), then sentence boundaries for oversized paragraphs.
- Target 600–1,200 tokens; hard cap 2,000 or 25% of the model context.
- Overlap one sentence at each boundary.
- Stable IDs: `c{chunk}`, `p{paragraph}`, `s{sentence}` derived from the original document, not chunk order.
- Re-review only dirty chunks: hash each paragraph, compare to stored hashes.
- Parallelism: max 3 chunk requests, global max 4.
- Cancellation: any new keystroke cancels the in-flight document review.

### 7.3 Prompt contract

System prompt (see [`PROMPTS.md`](PROMPTS.md) for the full text):

- Minimal edits only; smallest change that fixes the problem.
- `original` must be copied verbatim, including punctuation.
- One suggestion per problem; no overlapping edits.
- Omit when unsure.
- Explanation ≤20 words, naming the rule.
- Preserve names, numbers, dates, commitments.
- Output JSON only.

User message:

```text
<paragraph id="p3">
Their going to the conference next week , and they has a presentation.
</paragraph>
```

Dialect, enabled categories, style rules, and glossary are injected as a compact system preamble, not repeated per paragraph.

### 7.4 Output schema

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": ["suggestions"],
  "properties": {
    "suggestions": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["paragraph_id", "original", "replacement", "category", "severity", "explanation"],
        "properties": {
          "paragraph_id": { "type": "string" },
          "original": { "type": "string", "description": "verbatim substring, unique in the paragraph, max ~120 chars" },
          "replacement": { "type": "string", "description": "empty string means delete" },
          "category": { "type": "string", "enum": ["grammar","spelling","punctuation","style","clarity","conciseness","word_choice","consistency"] },
          "severity": { "type": "string", "enum": ["low","medium","high"] },
          "explanation": { "type": "string", "description": "one sentence, max 20 words" }
        }
      }
    }
  }
}
```

No offsets. No self-reported confidence. No free-form prose.

### 7.5 Offset resolution

For each suggestion:

1. **Schema validate**; reject invalid items.
2. **Find the paragraph** by `paragraph_id` in the chunk map.
3. **Exact match** `paragraph.indexOf(original)`.
4. **Normalized match**: NFC, collapse whitespace, normalize curly quotes/dashes, with an index map back to original UTF-16 offsets.
5. **Fuzzy match**: ±40-character window, diff-match-patch similarity ≥0.85.
6. **Reject** unresolved or ambiguous (multiple matches without a unique anchor).
7. **Post-checks**: replacement ≠ original; span ≤2× original or ≤200 chars; no overlap with a higher-severity accepted edit; explanation ≤25 words.
8. **Convert** chunk-relative to document-global; store UTF-16 and grapheme offsets.

Resolution quality is stored (`Exact` / `Normalized` / `Fuzzy`) and used for ranking. It is not shown as a fake confidence score.

### 7.6 Dedupe and merge

- Normalize to `(global_start, global_end, replacement)`.
- Exact duplicates: keep one; prefer higher severity; merge explanations.
- Overlapping spans:
  - same replacement → merge union if valid;
  - different → keep higher severity, then better resolution, then interior span.
- Local vs LLM overlap: local wins for spelling/punctuation/capitalization (deterministic); LLM wins for clarity/tone/style.
- Cap 8 suggestions per paragraph, 100 per document; sort severity then document order.
- Drop suggestions that land inside code blocks, URLs, email addresses, or inline code spans (Harper's parser marks these; for LLM, apply a regex/heuristic guard).

### 7.7 Repair loop

Budget: 2 extra calls maximum.

```text
call → parse (jsonrepair if needed) → schema validate
  fail → repair with validator error, temperature 0, max_tokens ×1.5
resolve → resolution_rate = resolved / total
  < 0.8 and unresolved > 0 → second repair with only unresolved originals + exact paragraph text
still failing → switch mechanism (json_schema → tool call → json_object)
still failing → switch model (cheap → mid)
finally → drop unresolved; surface "N suggestions could not be located"
```

Escalate to a better model when schema fails twice, resolution <0.6, or the user taps "looks wrong."

### 7.8 Caching

- Key: `SHA256(normalized_text || model || prompt_version || schema_version || params_hash)`.
- In-memory LRU by default; disk cache opt-in and encrypted or hash-only.
- TTL 24 h for cloud results; invalidate on prompt/schema version change.
- Never cache across providers/models.
- Never store raw text in the cache key or logs; use hashes.

### 7.9 Tone detection

- Only for 150–2,000 characters (matching Grammarly's useful range).
- Debounce 1.2 s; cache by text hash; cheap model; `temperature: 0`.
- Labels: neutral, confident, friendly, formal, informal, assertive, empathetic, urgent, cautious, enthusiastic.
- Emoji is derived locally from the label; do not ask the model for an emoji.

### 7.10 Rewrites

- Separate task, full-text output: `{ rewritten_text, changes[], meaning_preserved }`.
- Invariant checks: all numbers, dates, proper nouns, emails, URLs from the source must appear in the output.
- Length guard: ±15% for tone, ±40% for conciseness; reject outside the band and regenerate once.
- UI shows a word-level diff; accept per hunk or all.
- Never silently replace; always show the diff first.

### 7.11 Voice profiles (`elocutio-voice`)

Voice-aware rewriting is a retrieval + preference problem, not a fine-tuning problem. Full design: [`VOICE-LEARNING.md`](VOICE-LEARNING.md).

**Profile composition**

```text
uploaded samples ─┐
accepted/dismissed/edited suggestions ─┼─► feature extraction ─► sufficient statistics
explicit overrides/ratings ───────────┘        │
                                               ▼
                                      style card + exemplars
                                               │
                                               ▼
                                    prompt injection (rewrite/compose)
```

**Pipeline**

1. **Ingest:** `.txt`/`.md`/`.docx` or paste; parse locally; normalize; detect language; strip quoted replies; quality gate; dedupe by SHA-256.
2. **Extract:** rhythm (sentence/paragraph length, variance), diction (contractions, MATTR, preferred/avoided terms), structure (greeting, sign-off, bullets), punctuation (Oxford comma, em dash, semicolon, exclamation, emoji), tone (formality, warmth, hedging, assertiveness), person (first/second, passive).
3. **Estimate:** sufficient statistics + per-trait confidence (Beta/Wilson/SE); rhetorical moves (greeting, apology, request, sign-off, etc.) and signature 2–4-gram phrases with counts; explicit overrides win. Optional enrichment (Apple FM/Ollama, or cloud with per-sample consent) labels humor/empathy nuances; labels only, never rewritten samples.
4. **Update:** weighted events with a 180-day half-life; cap any single sample's influence; drift detection on a 30-day window.
5. **Select:** 2–4 diverse exemplars (40–120 words) per context; total prompt budget ≤500 tokens.
6. **Inject:** `<voice_profile>` in the system/user preamble for rewrite, compose, reply, and light for clarity; **off** for proofreading.
7. **Verify:** 8-gram overlap guard against samples; entity/number invariant; meaning preservation; reject and regenerate once on failure.

**Feedback weights**

| Event | Weight |
| --- | ---: |
| User-authored text (analyzed with consent) | 1.0 |
| User edit after accepting | 0.7 |
| Tone selection | 0.6 |
| Dismissal | 0.5 (negative) |
| Accepted AI text unchanged | 0.1 |
| Explicit trait override | ∞ |

Category multipliers: grammar/spelling/punctuation 0.3–0.4; clarity/conciseness 0.7; word choice/style/tone 1.0. Grammar dismissals become rule preferences, not general voice evidence.

**Storage:** samples encrypted at rest with AES-256-GCM; the data key is wrapped by the Keychain. "Features only" mode stores no raw text. Exemplars are user-reviewable excerpts. Deleting a sample cascades to its exemplars and stats contribution.

**Contexts:** email, chat, document, social, support, general. Multiple named profiles (Personal, Work, Brand). Per-app/site mapping. Brand profiles require a permission attestation.

---

## 8. Local engine

- `harper-core` for English: spelling, grammar, punctuation, capitalization, style, dialect.
- Personal dictionary and per-app ignore lists.
- Five dialects: US, UK, CA, AU, IN.
- Runs in the highlighter process, every keystroke, <10 ms for a paragraph.
- LLM can confirm/dismiss local findings but does not replace them.
- For non-English text, disable Harper (language detection via `whatlang` or a small model) and rely on the LLM.
- Harper's `Lint` model is mapped to our `Suggestion`; extend the highlighter to render both local and LLM suggestions with distinct colors and sources.

---

## 9. macOS platform layer

### 9.1 Read path

```text
AXUIElementCreateSystemWide()
  → kAXFocusedUIElementAttribute
  → kAXRoleAttribute / kAXSubroleAttribute
      - AXSecureTextField → abort
      - AXTextField/AXTextArea/AXComboBox/AXWebArea → continue
  → kAXSelectedTextAttribute (if non-empty)
  → else kAXValueAttribute
  → else kAXStringForRange around kAXSelectedTextRange
```

Rules:

- Set `AXUIElementSetMessagingTimeout(systemWide, 0.25)` once.
- Never call AX on the main thread; use a serial queue.
- `AXUIElement` is not `Sendable`; keep it on one thread or wrap carefully.
- Check `AXIsProcessTrusted()` for permission; never infer from `AXError` (untrusted returns `-25204`, not `-25211`).
- Bail on secure input: `AXSubrole == "AXSecureTextField"` or `IsSecureEventInputEnabled()`.
- Cap the read size (e.g., 20k characters) and use parameterized attributes for large docs.
- Skip code/URL/email spans when analyzing.

### 9.2 Write path ladder

```text
Re-read the field and compare to the analyzed text.
  mismatch → abort ("Text changed; refresh suggestions")
  secure   → abort
  ┌─────────────────────────────────────────────────────────────┐
  │ 1. AX fast path                                             │
  │    - AXUIElementIsAttributeSettable(kAXValueAttribute)      │
  │    - role is a native text role                             │
  │    - app is not Chromium-family                             │
  │    - field is short (< ~10k chars)                          │
  │    → splice value, write, restore caret, read back          │
  ├─────────────────────────────────────────────────────────────┤
  │ 2. Pasteboard + ⌘V (primary for Chromium/Electron)          │
  │    - save pasteboard items                                  │
  │    - write text with org.nspasteboard.TransientType +       │
  │      org.nspasteboard.ConcealedType                         │
  │    - synthesize ⌘V (keycode 9, .maskCommand)                │
  │    - wait 150–250 ms, restore by item if changeCount        │
  │      still matches                                          │
  │    - read back; if mismatch, report and offer undo          │
  ├─────────────────────────────────────────────────────────────┤
  │ 3. Give up gracefully                                       │
  │    - show "Couldn't edit this app; copy the suggestion"     │
  │    - never corrupt text                                     │
  └─────────────────────────────────────────────────────────────┘
```

Chromium/Electron includes Chrome, Edge, Arc, Slack, VS Code, Notion desktop, Discord, and most modern apps. In those, AX write is a no-op; paste is the only reliable path.

Clipboard safety:

- Tag the item transient and concealed so clipboard managers ignore it.
- Restore by item, not by string.
- If `changeCount` changed unexpectedly, leave the user's clipboard alone.
- Never paste into a password field; `IsSecureEventInputEnabled()` is the guard.

**M1 implementation status:**

- The explicit **Rewrite Selection** action (tray menu) reads `AXSelectedText`, calls the provider in the main process, writes the replacement to the pasteboard, synthesizes ⌘V, waits 180 ms, verifies the replacement in `AXValue`, and restores the previous clipboard text. Verified in TextEdit, Safari, and Chrome.
- The local-lint card still uses the AX fast path (`AXUIElementSetAttributeValue`) because it needs span-level replacement, not whole-selection paste.
- Secure input is refused via `IsSecureEventInputEnabled()` and `AXSubrole == AXSecureTextField`.
- **Not yet:** transient/concealed pasteboard types, item-level clipboard restore, global shortcut, and read-back diff/undo UI. Those are M1 hardening tasks.

Caret/selection:

- After paste, the target usually places the caret correctly; verify.
- After AX splice, set `kAXSelectedTextRangeAttribute` to `location + insertedLength`.
- Never whole-field replace in a long document.

### 9.3 Overlay

Suggestion card / widget:

```rust
// Native panel (if not using the inherited egui window manager)
panel.set_style_mask([.nonactivating_panel, .borderless, .full_size_content_view]);
panel.set_level(.floating);                 // CGLevel 3
panel.set_collection_behavior([
    .can_join_all_spaces,
    .full_screen_auxiliary,
    .transient,
    .ignores_cycle,
]);
panel.set_can_become_key(false);
panel.set_can_become_main(false);
panel.set_hides_on_deactivate(false);
panel.set_opaque(false);
panel.set_background_color(clear);
```

- Click-only: no text fields in the card. A custom-instruction input belongs in the assistant panel, which may become key and must restore focus afterward.
- Never call `makeKeyAndOrderFront`, `NSApp.activate`, or `orderFrontRegardless` on a panel that should not take focus. Audit every inherited call.
- Position using a single `cocoaRect(fromAX:)` helper (AX is top-left origin; Cocoa is bottom-left of the primary display). Do not convert per-screen.
- Hide on app switch, space change, screen lock, field invalidation, and window move; reposition on scroll after a debounce.

Underline overlay:

- One click-through window per screen, created once, at `CGWindowLevel(1500)` (`assistiveTechHigh`).
- `ignoresMouseEvents = true`; no key/main.
- Draw rounded rects from `AXBoundsForRange`; disable implicit animations (`CATransaction.setDisableActions(true)`).
- Compare rect sets within 0.5 pt before redrawing.
- Hide overlays during screen sharing if the user enables that preference.

### 9.4 Permissions and TCC

| Permission | When | Prompt |
| --- | --- | --- |
| Accessibility | Required for cross-app read/write | `AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt: true])` |
| Screen Recording | Optional OCR only | `CGRequestScreenCaptureAccess()` |
| Input Monitoring | Not planned | Use Carbon hotkeys / Tauri global shortcuts |
| Automation (Apple Events) | Not planned | — |

Deep links:

- Accessibility: `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`
- Screen Recording: `x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture`

TCC identity:

- Fixed bundle ID `app.elocutio.desktop`.
- Stable self-signed "Elocutio Dev" certificate for local builds.
- Developer ID for releases.
- A debug diagnostics screen shows `AXIsProcessTrusted`, bundle ID, code signature, and a `tccutil reset` hint.

### 9.5 Launch and lifecycle

- `LSUIElement = true`; `NSApp.setActivationPolicy(.accessory)` as a belt-and-braces measure.
- Menu-bar item with states.
- Login item via `SMAppService.mainApp` (macOS 13+) or Tauri autostart; verify the Tauri implementation uses the modern API.
- On launch: check permissions, restore highlighter, start bridge only if enabled, warm app catalog.
- On sleep/wake and app activation: revalidate the focused element and hide stale overlays.

### 9.6 Google Docs and other unsupported apps

- Google Docs uses canvas; no reliable DOM/AX text. Declare unsupported.
- Terminal/iTerm: read-only at most; never auto-inject.
- Password fields and secure input: disabled entirely.
- Remote desktops/games/Java Swing: unsupported.
- Show a per-app status in Settings and a "Why?" explanation.

---

## 10. IPC between main and highlighter

Harper already communicates with the highlighter subprocess. Extend the protocol, version it, and test it.

### 10.1 Framing

- Newline-delimited JSON over stdin/stdout.
- Each message has `v`, `type`, `id`.
- Max message size 2 MB; reject larger.
- Backpressure: highlighter drops stale results; main coalesces requests by app/field hash.

### 10.2 Messages

```json
{"v":1,"type":"analysis_request","id":"r1","app":{"bundle_id":"com.apple.mail","pid":123},"text":"...","paragraphs":[{"id":"p0","start":0,"end":42}],"options":{"dialect":"en-US","categories":["grammar","clarity"]},"mode":"field"}
{"v":1,"type":"analysis_result","id":"r1","suggestions":[...],"tone":null,"usage":{"input_tokens":600,"output_tokens":450,"cost_usd":0.00028}}
{"v":1,"type":"apply_request","id":"a1","suggestion_id":"s1"}
{"v":1,"type":"apply_result","id":"a1","status":"ok","verified":true,"method":"pasteboard"}
{"v":1,"type":"suggestion_event","id":"e1","kind":"accepted|dismissed|edited_after_accept","suggestion_id":"s1","app":"com.apple.mail","category":"clarity"}
{"v":1,"type":"state","id":"s1","focused":true,"app":"com.apple.mail","secure":false,"field_hash":"...","char_count":420}
{"v":1,"type":"config_changed","config":{...}}
```

The highlighter never receives API keys or full model configs beyond what it needs (categories, dialect, style rules).

### 10.3 Failure handling

- If the highlighter exits, main restarts it with exponential backoff (max 3), preserving user settings.
- If IPC stalls, main cancels the request and shows "Analysis unavailable."
- If the highlighter restarts, it re-reads the focused field and requests a refresh.

---

## 11. Browser bridge and extension

### 11.1 Why a bridge

Browsers are where people write, and AX is best-effort in Chromium. The extension is the reliable path for Gmail, Notion, Slack web, Linear, and plain textareas. It also renders precise underlines with the CSS Custom Highlight API.

The extension never holds API keys. It sends text to the local app; the app analyzes and returns suggestions.

### 11.2 Transport

**Phase 1 (M3): localhost WebSocket.**

- Bind `127.0.0.1:43110` (configurable; fail clearly if busy).
- Pairing: the app shows a 6-digit code; the user enters it in the extension popup. The extension receives a 256-bit token and stores it in `chrome.storage.local`.
- Every connection sends `Authorization: Bearer <token>` in the first message (WebSocket handshake headers are not reliably settable from extensions).
- Server checks `Origin` against `chrome-extension://<id>` and `safari-web-extension://<id>`.
- No remote access; no binding to `0.0.0.0`.
- Close the server when the app quits or the user disables the extension.

**Phase 2 (post-1.0): native messaging.**

- Add a native host manifest per browser; the host relays to the app over XPC or a Unix socket.
- Benefits: browser launches the host on demand; browser-enforced allowlist; no open port.
- Keep the WebSocket path for Safari and development.

### 11.3 Extension architecture

- Manifest V3; service worker ephemeral; state in `chrome.storage.session`/`local`.
- Content script in all frames (`all_frames: true`) for Gmail/Notion iframes.
- Closed shadow-root overlay for cards/underlines; CSS Custom Highlight API where available.
- Adapters:
  - `textarea`/`input`: mirror div for underlines; set `.value` + dispatch `input`.
  - `contenteditable`: Range + `document.execCommand('insertText')` (single undo, framework-friendly) with a DOM fallback + synthetic `InputEvent`.
  - Gmail: body contenteditable; subject line is out of scope (Grammarly doesn't check it either).
  - Notion/Slack/Linear: contenteditable heuristics; re-render on mutation; debounce.
  - Google Docs: detect and show "unsupported."
- Debounce 700 ms; send a `context` message with app/site, selection, and text; render returned suggestions.
- Never fetch remote code; all logic ships in the bundle.

### 11.4 Protocol

```json
{"v":1,"type":"hello","token":"...","client":"chrome","version":"0.1.0"}
{"v":1,"type":"analyze","id":"r1","site":"mail.google.com","text":"...","selection":{"start":10,"end":20},"options":{"categories":["grammar","clarity"]}}
{"v":1,"type":"result","id":"r1","suggestions":[...]}
{"v":1,"type":"event","id":"e1","kind":"accepted","suggestion_id":"s1"}
{"v":1,"type":"ping"}
```

### 11.5 Security

- Token stored only in the extension's storage and the app's Keychain.
- Origin allowlist; reject unknown origins even with a valid token.
- Rate limit: 20 requests/minute per extension.
- Never analyze fields with `type=password`, `autocomplete=current-password`, or `data-elocutio-ignore`.
- Do not inject into cross-origin iframes unless the adapter explicitly supports the site.

---

## 12. Persistence (`elocutio-store`)

SQLite via `rusqlite` (bundled). WAL mode. Migrations with `refinery` or a simple version table.

```sql
CREATE TABLE providers (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,              -- openrouter | openai | anthropic | gemini | ollama | custom
  name TEXT NOT NULL,
  base_url TEXT,
  keychain_ref TEXT,
  headers_json TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);

CREATE TABLE models (
  provider_id TEXT NOT NULL,
  model_key TEXT NOT NULL,
  display_name TEXT,
  context_length INTEGER,
  prompt_price REAL,               -- USD per 1M tokens
  completion_price REAL,
  capabilities_json TEXT,
  last_seen_at INTEGER,
  PRIMARY KEY (provider_id, model_key)
);

CREATE TABLE routes (
  task TEXT PRIMARY KEY,           -- proofread | tone | rewrite | chat | title
  model_ref TEXT NOT NULL,
  fallback_ref TEXT,
  temperature REAL,
  max_tokens INTEGER
);

CREATE TABLE app_rules (
  bundle_id TEXT PRIMARY KEY,
  mode TEXT NOT NULL,              -- off | local | llm | live
  widget_anchor_json TEXT,
  updated_at INTEGER NOT NULL
);

CREATE TABLE dictionary_entries (
  word TEXT PRIMARY KEY,
  case_sensitive INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE TABLE style_rules (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,              -- term | regex | preferred | tone
  pattern TEXT,
  replacement TEXT,
  severity TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  source TEXT
);

CREATE TABLE snippets (
  id TEXT PRIMARY KEY,
  trigger TEXT UNIQUE,
  body TEXT NOT NULL,
  tags TEXT
);

CREATE TABLE usage (
  id TEXT PRIMARY KEY,
  provider_id TEXT,
  model_key TEXT,
  task TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cost_usd REAL,
  latency_ms INTEGER,
  created_at INTEGER NOT NULL
);

CREATE TABLE request_log (
  id TEXT PRIMARY KEY,
  host TEXT NOT NULL,
  model_key TEXT,
  purpose TEXT,
  status TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cost_usd REAL,
  created_at INTEGER NOT NULL
);                                  -- no text, no key

CREATE TABLE provenance (
  id TEXT PRIMARY KEY,
  app_bundle_id TEXT,
  event_type TEXT NOT NULL,        -- typed | pasted | ai_inserted | ai_replaced | accepted | dismissed
  content_hash TEXT,
  metadata_json TEXT,
  created_at INTEGER NOT NULL
);                                  -- no plaintext by default

CREATE TABLE documents (
  id TEXT PRIMARY KEY,
  title TEXT,
  content TEXT,                    -- only for the local Docs editor
  created_at INTEGER,
  updated_at INTEGER
);

CREATE TABLE voice_profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  context TEXT NOT NULL,            -- email | chat | document | social | support | general
  language TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  strength TEXT NOT NULL DEFAULT 'balanced',
  summary TEXT,
  traits_json TEXT,
  overrides_json TEXT,
  stats_json TEXT,
  confidence REAL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE voice_samples (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES voice_profiles(id) ON DELETE CASCADE,
  source TEXT NOT NULL,             -- upload | paste | clipboard
  filename TEXT,
  content_hash TEXT NOT NULL UNIQUE,
  language TEXT,
  word_count INTEGER,
  text_encrypted BLOB,              -- AES-256-GCM; null in features-only mode
  excerpt TEXT,
  quality REAL,
  sensitive INTEGER DEFAULT 0,
  created_at INTEGER NOT NULL,
  deleted_at INTEGER
);

CREATE TABLE voice_exemplars (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES voice_profiles(id) ON DELETE CASCADE,
  sample_id TEXT REFERENCES voice_samples(id) ON DELETE CASCADE,
  text TEXT NOT NULL,
  score REAL,
  created_at INTEGER NOT NULL
);

CREATE TABLE voice_moves (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES voice_profiles(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,               -- greeting | signoff | apology | request | disagreement | gratitude | followup | bad_news | praise | scheduling
  phrase TEXT NOT NULL,
  count INTEGER NOT NULL,
  confidence REAL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);

CREATE TABLE voice_events (
  id TEXT PRIMARY KEY,
  profile_id TEXT,
  kind TEXT NOT NULL,               -- accepted | dismissed | edited_after_accept | rated | tone_selected | override
  suggestion_id TEXT,
  category TEXT,
  context TEXT,
  signal REAL,
  metadata_json TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE voice_contexts (
  profile_id TEXT NOT NULL REFERENCES voice_profiles(id) ON DELETE CASCADE,
  bundle_id TEXT,
  site TEXT,
  context TEXT NOT NULL,
  PRIMARY KEY (profile_id, bundle_id, site)
);
```

Indexes: `usage(created_at)`, `request_log(created_at)`, `provenance(created_at)`, `models(provider_id)`, `voice_samples(profile_id)`, `voice_events(created_at)`, `voice_exemplars(profile_id)`.

Privacy defaults:

- No full text in logs or usage.
- Provenance off until enabled.
- Cache in memory only unless the user opts into disk cache.
- "Clear data" deletes usage, logs, provenance, documents, voice samples/profiles, and cache; keys are deleted separately with confirmation.
- Voice samples are encrypted at rest; only the style card and selected excerpts are sent to a provider, and the UI previews them.

---

## 13. UI architecture

### 13.1 Surfaces

| Surface | Technology | Notes |
| --- | --- | --- |
| Overlay underlines | Rust/egui (inherited highlighter) | Click-through, level 1500 |
| Widget | Rust/egui or native NSPanel | Non-activating; count + tone |
| Suggestion card | Rust/egui or native NSPanel | Click-only; diff; accept/dismiss |
| Assistant panel | Tauri webview | Chat, rewrite, compose; may take focus |
| Settings | Tauri webview | Providers, models, writing, voice, apps, privacy, usage |
| Voice editor | Tauri webview | Profiles, samples, learned traits/moves, preview, payload |
| Onboarding | Tauri webview | 6–8 steps; playground; optional voice samples |
| Menu bar | Tauri tray | Native menu |

If the inherited egui overlay cannot meet the visual bar, replace the widget/card with native AppKit panels and keep egui only for underlines. Decide in M2 with a design spike.

### 13.2 Design tokens

Brand tokens live in `assets/brand/tokens.json`; the full system is in [`BRAND.md`](BRAND.md).

- **Typeface:** Inter (400/500/600), with system fallbacks; native menus may use SF Pro on macOS.
- **Palette:** Ink Indigo `#191F70` (light accents), Soft Indigo `#B8BEFF` (dark accents), Near Black `#171717` (dark surface), Warm Paper `#F8F7F3` (light surface), White `#FFFFFF` (cards/reversed text). Never Ink Indigo on dark.
- **Semantic colors:** error red, clarity blue, tone purple, style gray; always pair color with labels/icons.
- **Materials:** `NSVisualEffectView` on macOS 14/15; `NSGlassEffectView` on macOS 26 when available.
- **Motion:** 150 ms ease-out; respect Reduce Motion.
- **Density:** compact; the card is ≤320 pt wide.
- **Accessibility:** VoiceOver labels, keyboard navigation, focus rings, ≥4.5:1 contrast.

### 13.3 State model

```text
Idle → Focused → Debouncing → AnalyzingLocal → AnalyzingLLM → Suggestions → Applying → Idle
         │                          │                │
         └────────── error ─────────┴────────────────┘
```

The widget reflects the state with a subtle indicator; errors never block typing.

---

## 14. Security and privacy

### 14.1 Trust boundaries

| Boundary | Data | Protection |
| --- | --- | --- |
| Main ↔ Keychain | API keys | Data Protection Keychain, device-only, non-sync |
| Main ↔ provider | text chunks, prompts | HTTPS; user-configured host; no proxies by default |
| Main ↔ highlighter | text, suggestions | Local stdin/stdout; no network |
| Main ↔ extension | text, suggestions, token | Loopback WS; pairing token; Origin allowlist |
| Main ↔ disk | usage, config, rules | File permissions 0600; no text by default |

### 14.2 Threats

| Threat | Mitigation |
| --- | --- |
| Malicious page connects to bridge | Token + Origin allowlist + loopback bind |
| Stolen key | Keychain device-only, optional biometric gate, key never logged |
| Prompt injection in analyzed text | Text-as-data delimiters; no tools in analysis; structured output only; never execute instructions from text |
| Malicious update | Minisign/EdDSA signature; HTTPS; verify before install |
| Local process reads DB | 0600 permissions; no text stored; optional encryption later |
| Overlay clickjacking | Click-through underlines; cards have explicit hit targets; no arbitrary web content |
| Clipboard leak | Transient/concealed types; restore; never leave replacement text on the clipboard |
| Screen recording | OCR opt-in only; overlays hidden during screen share if enabled |
| Voice samples leak via prompts | Excerpt-only, user-reviewed; "style card only" mode; preview; redaction option |
| Voice samples stolen from disk | AES-256-GCM at rest; key wrapped by Keychain; FileVault recommended |
| Model copies sample content | 8-gram overlap guard; reject/regenerate; never inject full samples |
| Profile drifts toward AI voice | Learn from user-authored text/edits; down-weight unedited AI acceptances |
| Profile used to impersonate someone | Consent attestation; brand-profile labeling; no "clone a person" feature |

### 14.3 Verifiable claims

Say **"no first-party inference backend"**, not "no network calls." The app does make network calls to the user's provider and to the update server. Back the claim with:

- open-source code,
- an in-app request log (host, model, tokens, cost),
- a host allowlist in Settings,
- no telemetry,
- a reproducible build.

---

## 15. Performance budgets

| Path | Budget | Technique |
| --- | --- | --- |
| AX focus read | <15 ms | Serial queue, timeout, role gate |
| Local lint (paragraph) | <10 ms | harper-core, incremental |
| IPC round trip | <5 ms | Newline JSON, coalescing |
| Debounce | 700 ms | Per-field timer |
| First LLM token | <800 ms p50 | Streaming, cheap model, no reasoning |
| Full suggestions (150 words) | <2.5 s p95 | Chunking, parallel, cache |
| Accept + verify | <120 ms | Pasteboard path, 150 ms restore delay |
| Widget show | <80 ms | Pre-created windows |
| Idle CPU | <0.5% | No polling faster than 250 ms; back off when inactive |
| Voice extraction | <2 s per 1,000 words | Local Rust; no network |
| Voice prompt overhead | +300–500 tokens | Card + 2–4 exemplars; capped |
| Memory | <220 MB | Bounded caches; release AX trees |

---

## 16. Testing

### 16.1 Unit

- Resolver: exact/normalized/fuzzy, Unicode, emoji, CRLF, combining marks, RTL, CJK, ambiguous duplicates; property-based (`proptest`).
- Chunker: boundaries, overlap, max size, no split inside grapheme clusters.
- Schema: missing/extra fields, empty replacement, no-op, overlap.
- Adapters: recorded fixtures for OpenRouter/Anthropic/Gemini; streaming; errors; capability checks.
- Cost: model pricing, cache tokens, long-context overrides.
- Keychain: store/read/delete; no secret in logs.
- Merge: local vs LLM overlaps; severity; dedupe.
- IPC: framing, version mismatch, oversized messages, restart.
- Voice extraction: rhythm/diction/structure/punctuation/tone on golden samples; confidence; language detection.
- Voice profile: decay, overrides, drift detection, per-context selection, style-card summary.
- Voice ingestion: txt/md/docx parsing, quality gate, dedupe, quoted-reply stripping, size limits.
- Voice copy guard: 8-gram overlap rejection; entity/number invariants.
- Voice feedback weights: AI-text down-weighting, preference thresholds.

### 16.2 Integration

- `tools/fake-provider` (axum): deterministic responses; slow/429/malformed/partial stream; schema failure; unresolved spans.
- End-to-end: fixture text → analyze → accept → verify final text.
- Bridge: pairing, bad token, bad origin, reconnect, backpressure.
- Updater: signature verification, manifest parsing.
- Highlighter restart: settings preserved; focus reacquired.
- Voice end-to-end: upload 3 samples → profile → preview rewrite → rate → delete sample → derived data gone.
- Voice encryption: DB copy without the Keychain key reveals no sample text.
- Voice style-card-only: request payload contains no excerpt text.

### 16.3 Manual matrix

- 12 Tier A + 6 Tier B apps.
- macOS current + previous; light/dark; increased contrast; Reduce Motion.
- Retina/non-Retina; two monitors; scaling; Spaces; full-screen; Stage Manager.
- Text: empty, 1 char, 10k, emoji, RTL, CJK, code, markdown, rich links.
- Permissions: fresh, revoked mid-session, upgrade, second user.
- Network: offline, slow, captive portal, proxy, VPN.
- Security: secure field, password manager, terminal, screen sharing.
- Voice: upload 3 samples per context; check extracted text, traits, preview, deletion; verify style-card-only sends no excerpts.

### 16.4 Quality

- Golden set 200–500 items; span F0.5, exact acceptance, ERRANT M2, GLEU, meaning preservation, over-edit rate, resolution rate, latency, cost.
- Regression gate in CI on prompt/model changes.
- Voice: held-out trait similarity, "sounds like me" rating, blind A/B, copy guard, meaning preservation, feedback-loop drift.
- Human review during beta.

---

## 17. Build and release

### 17.1 Development

- `just` or `make` targets: `dev`, `test`, `lint`, `build`, `sign`, `notarize`, `release`.
- `scripts/dev-sign.sh` creates/uses a stable self-signed certificate and fixed bundle ID.
- `cargo fmt`, `cargo clippy -D warnings`, `eslint`, `prettier`.
- CI: macOS runner, cache Cargo/pnpm, run tests, build app; release workflow on tag.

### 17.2 Signing and notarization

1. Build universal (arm64 + x86_64) release.
2. Sign the `.app` and embedded binaries with Developer ID Application, hardened runtime, timestamp.
3. `xcrun notarytool submit --wait`.
4. `xcrun stapler staple`.
5. `spctl --assess --type execute`.
6. Create DMG, sign, notarize, staple.
7. Generate Tauri updater artifacts and signatures.

Entitlements: no App Sandbox. Avoid unnecessary entitlements. Do not disable library validation unless a dependency requires it; document if so.

### 17.3 Updates

- Tauri updater with minisign; public key in `tauri.conf.json`.
- Endpoint: `https://updates.elocutio.app/{{target}}/{{arch}}/{{current_version}}`.
- Stable/beta channels.
- Manual "Check for updates" plus background check.
- Rollback: keep the previous appcast entry and DMG.

### 17.4 Distribution

- GitHub Releases, Homebrew Cask, direct DMG.
- Checksums + signed manifest.
- No Mac App Store.
- Crash reports opt-in, scrubbed.

---

## 18. Spikes before committing (M0)

| # | Spike | Pass condition | Fallback |
| --- | --- | --- | --- |
| 1 | Does the inherited highlighter create truly non-activating panels? | Clicking a card does not move the caret; paste lands correctly in 5 apps | Patch with `objc2` NSPanel; or native AppKit card |
| 2 | Pasteboard + ⌘V in Chrome, Slack, VS Code, Notion | Replacement verified in all four | Per-app adapters or declare unsupported |
| 3 | AX read in AppKit + Chromium + Electron | Text and selection readable; secure fields rejected | Extension for web; matrix |
| 4 | Google Docs | Confirm canvas, no DOM text | Declare unsupported |
| 5 | Harper build in our workspace | Clean build <15 min; tests pass | Extract smaller workspace; pin crates |
| 6 | Tauri updater + notarization | Signed DMG installs and updates on a clean Mac | Sparkle or manual update |
| 7 | LLM latency end-to-end | Haiku p50 <2 s for 150 words | Cheaper model/streaming/caching |
| 8 | Keychain Data Protection | Key stored, readable after restart, not in login keychain | Security framework wrapper |
| 9 | Voice ingestion + copy guard | 3 docx/txt samples → style card → preview rewrite with 0 eight-gram overlaps | Feature-based profile only; PDF later |

If spikes 1–3 fail, switch to Option B (native Swift) before building the LLM layer deeply.

---

## 19. Open technical questions

1. Does the inherited highlighter use `NSPanel`/non-activating semantics, or egui windows that activate? Verify in M0.
2. Can we reuse Harper's `Lint` rendering pipeline for LLM suggestions, or do we need a parallel overlay model?
3. Does Tauri's global-shortcut plugin require Input Monitoring? Prefer Carbon hotkeys if it does.
4. How stable is `harper-core`'s public API across minor versions? Pin and test.
5. What is the exact Screen Recording re-prompt cadence on macOS 26? Keep OCR opt-in regardless.
6. Does Safari's `SafariWebExtensionHandler` perform well enough for per-keystroke messaging? Defer Safari.
7. Does `AXEnhancedUserInterface` still force Chromium's accessibility tree on macOS 26? Test in the M0 spike.
