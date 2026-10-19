# LLM Provider Layer — Research Report

**Project:** open-source Grammarly alternative (macOS, bring-your-own-key, default OpenRouter)
**Research date:** 2026-10-19
**Author:** backend/AI engineering research pass

**How this was verified.** `web_search` is unavailable in this environment (its endpoint returns 404), so every fact below was taken from pages fetched directly with `curl`/`web_fetch` on 2026-10-19, plus one live call to OpenRouter's public model catalog (`GET https://openrouter.ai/api/v1/models`, 469 models returned). Sources are linked inline. Where a number is an engineering estimate rather than a documented value, it is labelled **[estimate]**. Where a claim could not be re-verified against a primary source in this pass, it is labelled **[verify before shipping]**.

---

## 0. TL;DR — recommended shape

| Decision | Recommendation |
|---|---|
| Default provider | OpenRouter, `https://openrouter.ai/api/v1` |
| Default model | `anthropic/claude-haiku-5.5` (structured outputs, 1M ctx, $0.10/$0.50 per M, reasoning can be disabled) |
| Fast fallback / alternate default | `google/gemini-3.1-flash-lite` ($0.25/$1.50) or `openai/gpt-5-mini` ($0.25/$2.00, but reasoning is mandatory) |
| Escalation model | `anthropic/claude-sonnet-5` ($2.00/$10.00) then `openai/gpt-5.6-sol` / `claude-opus-5.5` ($4/$20) |
| Abstraction | One canonical request/response model + thin per-provider adapters. OpenAI Chat Completions is the *base dialect*, not the internal model. |
| Structured output | `response_format: json_schema` with `strict: true` + `provider.require_parameters: true`; tool-call fallback; JSON-mode + repair loop for local/legacy models |
| Span strategy | Model returns **quoted original text + paragraph anchor**, never offsets. Client resolves offsets, validates by exact/fuzzy match, drops unverifiable edits. |
| Cost (1,000 reviews) | 150-word email: **≈$0.28** on Haiku 5.5, **≈$1.14** blended cheap-first/retry. 1,500-word doc: **≈$1.06**, **≈$4.24** blended. |
| Privacy | macOS Data Protection keychain (`kSecUseDataProtectionKeychain`), `WhenUnlockedThisDeviceOnly`, non-synchronizable; `provider.zdr: true` + `data_collection: "deny"`; local-only via Ollama/LM Studio. |

---

## 1. OpenRouter API

### 1.1 Base URL and endpoints

Base: `https://openrouter.ai/api/v1` — [API overview](https://openrouter.ai/docs/api_reference/overview).

| Endpoint | Purpose | Auth |
|---|---|---|
| `POST /chat/completions` | Chat / completions (the workhorse) | API key |
| `GET /models` | Full catalog + metadata | none required |
| `GET /model/{author}/{slug}` | Single model lookup (resolves aliases) | none required |
| `GET /models/{author}/{slug}/endpoints` | Per-provider endpoints, per-provider pricing | none required |
| `GET /key` | Current key: label, limit, usage, budgets | API key |
| `GET /generation?id=…` | Post-hoc cost/token stats for one generation | API key |
| `GET /credits` | Remaining account credits | **Management** key |
| `GET/POST/PATCH/DELETE /keys` | Key provisioning | **Management** key |
| `POST /messages` | Anthropic Messages-compatible skin | API key |
| `POST /responses` | OpenAI Responses-compatible skin | API key |

The OpenAI- and Anthropic-shaped skins are a 2025–2026 addition; they accept the native payloads and are documented alongside Chat Completions. For a new app, target `/chat/completions` for breadth and keep `/messages` in mind only if you need Anthropic-specific block semantics.

### 1.2 Auth headers

```http
Authorization: Bearer <OPENROUTER_API_KEY>
Content-Type: application/json
HTTP-Referer: https://yourapp.example      # optional, app attribution/rankings
X-OpenRouter-Title: YourAppName            # optional; legacy X-Title still accepted
```

- `X-OpenRouter-Title` is the current header; the docs explicitly say **`X-Title` is also accepted**. Use the new name, keep the old one only if you support very old proxies.
- `X-OpenRouter-Categories` assigns marketplace categories (app-attribution feature).
- `X-OpenRouter-Metadata: enabled` / `X-OpenRouter-Experimental-Metadata: enabled` opt in to routing metadata (`openrouter_metadata`) on responses and errors.
- `x-session-id` (or body `session_id`, ≤256 chars) pins sticky routing and groups logs.

Source: [Authentication](https://openrouter.ai/docs/api_reference/authentication), [API overview › Headers](https://openrouter.ai/docs/api_reference/overview).

⚠️ **Desktop-app note:** a user's OpenRouter key is more powerful than a normal model key — it can be used for OAuth and key management. Treat it as a credential, never ship a shared key in the binary, and never put an unlimited key in client-side code (the docs make this warning explicitly).

### 1.3 Request shape (`POST /chat/completions`)

Minimal subset relevant to us:

```jsonc
{
  "model": "anthropic/claude-haiku-5.5",
  "messages": [
    { "role": "system", "content": "…" },
    { "role": "user", "content": "…" }
  ],
  "max_tokens": 1024,
  "temperature": 0,
  "stream": false,
  "stop": ["\n\n###"],
  "response_format": { "type": "json_schema", "json_schema": { "name": "…", "strict": true, "schema": { } } },
  "tools": [{ "type": "function", "function": { "name": "…", "description": "…", "parameters": { } } }],
  "tool_choice": "auto",
  "reasoning": { "effort": "none" },
  "provider": { "order": ["anthropic"], "allow_fallbacks": true, "sort": "price",
                "data_collection": "deny", "zdr": true, "require_parameters": true },
  "seed": 42,
  "user": "stable-end-user-id",
  "session_id": "doc-123-review-1"
}
```

Notes from the schema:
- `max_tokens` range is `[1, context_length)`; if omitted you get the model default.
- `temperature` range `[0, 2]` at the OpenRouter layer — but the upstream model may clamp/reject it (see §2).
- `messages[].content` may be a string or an array of `{type:"text"}` / `{type:"image_url"}` parts. Image parts are only valid on the `user` role per the documented type.
- `name` on a message is prepended as `{name}: {content}` for non-OpenAI models.
- Unknown/unsupported parameters are **silently ignored** unless `provider.require_parameters: true`.

### 1.4 Response shape

```jsonc
{
  "id": "gen-…",
  "object": "chat.completion",
  "created": 1790000000,
  "model": "anthropic/claude-haiku-5.5",
  "choices": [{
    "index": 0,
    "message": { "role": "assistant", "content": "…", "tool_calls": [], "reasoning": "…" },
    "finish_reason": "stop",            // normalized: stop | length | tool_calls | content_filter | error
    "native_finish_reason": "end_turn"  // raw upstream value
  }],
  "usage": {
    "prompt_tokens": 812,
    "completion_tokens": 340,
    "total_tokens": 1152,
    "prompt_tokens_details": { "cached_tokens": 640, "cache_write_tokens": 0 },
    "completion_tokens_details": { "reasoning_tokens": 0 },
    "cost": 0.000412,
    "is_byok": false,
    "cost_details": { "upstream_inference_prompt_cost": 0.000081,
                      "upstream_inference_completions_cost": 0.000331 }
  }
}
```

Key facts:
- `usage` is **always** returned for non-streaming, and is computed with the **model's native tokenizer**, not a shared tokenizer. Do not estimate cost from your own tokenizer.
- `usage.cost` (credits) and `cost_details` are OpenRouter extensions; they make per-review cost accounting trivial.
- `finish_reason` is normalized; `native_finish_reason` preserves the provider's value.
- `provider` is returned on streaming chunks and mid-stream errors, not on the non-streaming top level in the documented schema; to know which provider served a request, use `X-OpenRouter-Metadata` routing metadata or query `/generation`.

Source: [API overview › Responses](https://openrouter.ai/docs/api_reference/overview).

### 1.5 Streaming (SSE)

- `stream: true` → `text/event-stream`.
- Frames are `data: {json}` lines terminated by `data: [DONE]`.
- The stream **occasionally contains SSE comments** such as `: OPENROUTER PROCESSING`. A hand-rolled parser must skip lines beginning with `:` before `JSON.parse` or it will crash. Use `eventsource-parser` if possible.
- The final chunk before `[DONE]` carries `usage`. Unlike OpenAI, OpenRouter's final chunk has a **non-empty `choices` array**: a content-free delta that repeats `finish_reason`.
- **Mid-stream errors** arrive as a normal SSE event on an HTTP `200` (headers were already sent):

```jsonc
data: {"id":"gen-abc","object":"chat.completion.chunk","created":1234567890,
 "model":"openai/gpt-4o","provider":"OpenAI",
 "error":{"code":429,"message":"Rate limit exceeded",
          "metadata":{"error_type":"rate_limit_exceeded","provider_code":"rate_limited"}},
 "choices":[{"index":0,"delta":{"content":""},"finish_reason":"error"}]}
```

So a robust client must treat `finish_reason: "error"` (and the presence of a top-level `error`) as a failure even though HTTP said 200. OpenRouter also documents that a non-streaming request can return `200 OK` with **only** an `error` object and no `choices` if the provider fails after headers — check the body, not just the status.

Source: [Streaming](https://openrouter.ai/docs/api_reference/streaming), [Errors › Streaming error formats](https://openrouter.ai/docs/api_reference/errors-and-debugging).

### 1.6 Error format and codes

Envelope:

```jsonc
{ "error": { "code": 429, "message": "Rate limit exceeded",
             "metadata": { "error_type": "rate_limit_exceeded", "provider_code": "rate_limited" } } }
```

HTTP status matches `error.code` for request-level failures. Documented codes:

| Code | Meaning | Retry? |
|---|---|---|
| 400 | Bad request / invalid params / CORS | no |
| 401 | Invalid credentials (bad or disabled key, expired OAuth) | no |
| 402 | Insufficient credits (account, key limit, or in-flight budget) | conditional — see below |
| 403 | Forbidden (permissions, guardrail block, moderation, budget) | no |
| 408 | Request timeout | yes |
| 429 | Rate limited (OpenRouter or upstream provider) | yes |
| 502 | Model down / invalid upstream response | yes |
| 503 | No provider meets routing requirements | yes (or relax routing) |

Additional error detail:
- `error.metadata.error_type` is the **stable, normalized vocabulary** to branch on (e.g. `rate_limit_exceeded`, `server`). It is present on non-streaming bodies, mid-stream chunks, and on the Anthropic/Responses skins (in different wire locations).
- On 500-class errors the message is generic and `provider_code` is omitted (to avoid leaking upstream details), but `error_type` remains.
- Moderation errors carry `metadata.reasons`, `metadata.flagged_input` (≤100 chars), `provider_name`, `model_slug`.
- `Retry-After` is sent on 429, 503, and on 402 whose `metadata.limit_source == "openrouter_in_flight_budget"`.

Source: [Errors and debugging](https://openrouter.ai/docs/api_reference/errors-and-debugging).

### 1.7 `/models` and model metadata

`GET /models` returns:

```jsonc
{
  "data": [{
    "id": "anthropic/claude-haiku-5.5",
    "canonical_slug": "…",
    "name": "Anthropic: Claude Haiku 5.5",
    "created": 1780000000,
    "description": "…",
    "context_length": 1000000,
    "architecture": {
      "input_modalities": ["text","image","file"],
      "output_modalities": ["text"],
      "modality": "text+image+file->text",
      "tokenizer": "Claude",
      "instruct_type": null
    },
    "pricing": {
      "prompt": "0.0000001",        // USD per token — multiply by 1e6 for $/M
      "completion": "0.0000005",
      "request": "0",
      "image": "0",
      "web_search": "0",
      "internal_reasoning": "0",
      "input_cache_read": "0.00000001",
      "input_cache_write": "0.000000125",
      "overrides": [ { "min_prompt_tokens": 100000, "prompt": "0.0000005", "completion": "0.0000025" } ]
    },
    "top_provider": { "context_length": 1000000, "max_completion_tokens": 128000, "is_moderated": false },
    "supported_parameters": ["tools","tool_choice","structured_outputs","response_format","reasoning","max_tokens","stop","temperature","top_p","seed","verbosity"],
    "reasoning": { "mandatory": false, "default_enabled": true, "default_effort": "medium",
                   "supported_efforts": ["max","xhigh","high","medium","low","none"] },
    "expiration_date": null,
    "benchmarks": { "design_arena": [ … ] }
  }],
  "total_count": 469,
  "links": { "next": null }
}
```

Practical details:
- **Pricing is a string of USD per token.** `"0.0000001"` = $0.10 per 1M input tokens. Parse as decimal, not float, if you display money.
- **`pricing.overrides`** encodes conditional pricing: `min_prompt_tokens` (long-context surcharge) and `utc_start`/`utc_end`/`utc_days` (peak/off-peak windows). The top-level keys reflect *current* default conditions; overrides carry exceptions. If you show a price, either show "from $X" or evaluate the override for the user's actual prompt size.
- `top_provider.max_completion_tokens` is a ceiling for `max_tokens`, not a guarantee; effective max output is limited by remaining context.
- `supported_parameters` is the authoritative capability list per model (`tools`, `structured_outputs`, `response_format`, `reasoning`, `stop`, `seed`, `verbosity`, …). Gate UI on it.
- `reasoning` metadata is new and important for UI: `mandatory` (cannot disable), `supported_efforts`, `default_effort`, `default_enabled`.
- Query params: `output_modalities=text|image|video|audio|speech|transcription|embeddings|rerank|decisions|all` (default returns **text-output models only**), `supported_parameters=tools`, `sort=pricing-low-to-high|…|most-popular|newest`, `offset`/`limit` (pagination is opt-in; limit defaults to 500, max 1000 when paginating).
- **`GET /models/{author}/{slug}/endpoints`** is the per-provider view and is where you find provider-specific context limits, `max_completion_tokens`, and **per-provider prices** — which can differ materially from the model-level price. Example observed: `openai/gpt-5-mini` model-level $0.25/$2.00 per M, but one endpoint priced $0.125/$1.00 per M. Use endpoints when routing/estimating precisely.

Source: [Models guide](https://openrouter.ai/docs/guides/overview/models), live catalog fetch 2026-10-19.

### 1.8 Provider routing fields

All under the top-level `provider` object:

| Field | Type | Default | Effect |
|---|---|---|---|
| `order` | `string[]` | — | Try these provider slugs in order. **Disables load balancing** and disables sticky routing. |
| `allow_fallbacks` | `bool` | `true` | Allow backup providers if the primary is unavailable. Set `false` for strict reproducibility. |
| `require_parameters` | `bool` | `false` | Route only to providers that support **every** parameter in the request. This is the switch that makes `structured_outputs` a hard requirement. |
| `data_collection` | `"allow" \| "deny"` | `"allow"` | `deny` = only providers that do not store data non-transiently and do not train on it. |
| `zdr` | `bool` | — | Route only to Zero Data Retention endpoints. ORs with account/guardrail ZDR. |
| `sort` | `"price"\|"throughput"\|"latency"` or `{by, partition}` | — | Explicit sort; disables load balancing. `partition: "none"` sorts endpoints globally across all models in `models[]`. |
| `only` / `ignore` | `string[]` | — | Allowlist / denylist of provider slugs. |
| `quantizations` | `string[]` | — | e.g. `["int4","int8","fp8"]`. |
| `max_price` | object | — | **Hard** cap; request fails if no endpoint is at/below it (unlike the soft performance prefs). |
| `preferred_min_throughput` | number \| `{p50,p75,p90,p99}` | — | Soft preference; below-threshold endpoints are deprioritized, not removed. |
| `preferred_max_latency` | number \| `{…}` | — | Soft preference, seconds. |
| `enforce_distillable_text` | `bool` | — | Only models that allow text distillation. |

Important nuances:
- Default routing **load-balances by price** (weighted by inverse square of price) among healthy providers, with fallback on 5xx/429.
- Even with `require_parameters: false`, there is a **soft preference** for providers supporting `tools`, `response_format`, and `verbosity`: if *some* providers support the parameter, the request is routed only to those; if *none* do, the parameter is ignored rather than failing. For structured output this is exactly the failure mode you must prevent → set `require_parameters: true` **and** check `supported_parameters` before sending.
- `order` is applied per model; `sort` applies to the candidate set.

Source: [Provider routing](https://openrouter.ai/docs/guides/routing/provider-selection).

### 1.9 Structured outputs

Two modes:

```jsonc
{ "response_format": { "type": "json_object" } }                       // JSON mode
{ "response_format": { "type": "json_schema", "json_schema": {
    "name": "grammar_suggestions", "strict": true, "schema": { … } } }  // schema mode
```

- Support is **per endpoint**, not per model: the same model served by different providers may or may not support it, and support can change. Check `supported_parameters` / the model page's Providers section, and send `provider.require_parameters: true`.
- `strict: true` means different things per provider: some enforce via constrained decoding/grammar, others *translate* your schema to their own format, others treat it as a strong hint. Exact compliance is **not** guaranteed on every endpoint (docs say so explicitly). Treat the response as untrusted JSON and validate.
- Strict modes restrict JSON Schema features. Assume the portable subset: `type`, `properties`, `required`, `additionalProperties: false`, `items`, `enum`, `description`, `anyOf`, `$defs`/`$ref`. Avoid `minLength`, `maxLength`, `pattern`, `format`, `minimum`, `maximum`, `minItems`, `maxItems`, `uniqueItems` in the schema itself; enforce those client-side. (Behaviour differs by provider — verify per model. **[verify before shipping]** for the exact per-provider keyword table.)
- `plugins: [{ "id": "response-healing" }]` performs automatic JSON repair on **non-streaming** `json_schema` requests. Useful as a safety net for weaker endpoints, but it doesn't fix semantically wrong spans — keep your own validation.
- Streaming + `json_schema` sends **partial JSON** that only becomes valid at the end; don't parse intermediate chunks as JSON.

Source: [Structured outputs](https://openrouter.ai/docs/guides/features/structured-outputs).

### 1.10 Tool calling

- OpenAI-shaped `tools` / `tool_choice`; normalized across providers. For providers with an OpenAI-compatible interface the payload is passed through; for others OpenRouter transforms it; and if a model has no tool support at all, OpenRouter **turns the tools into a YAML template and the model answers in an assistant message** (i.e. tool calls become prompt-encoded, not native). That fallback is unreliable for strict machine parsing — for structured extraction prefer `response_format` and only use tools where `supported_parameters` includes `tools`.
- `tool_choice`: `"none" | "auto" | {type:"function", function:{name}}`.
- Response: `choices[0].message.tool_calls[]` with `{id, type:"function", function:{name, arguments}}` where `arguments` is a **JSON string** you must parse.
- Anthropic and Google map tool definitions to their native tool-use blocks; on the OpenRouter layer you still read OpenAI-shaped `tool_calls`.

Source: [Tool calling](https://openrouter.ai/docs/guides/features/tool-calling).

### 1.11 Reasoning models and reasoning tokens

Unified `reasoning` object:

```jsonc
{ "reasoning": { "effort": "none" } }          // OpenAI-style: max|xhigh|high|medium|low|minimal|none
{ "reasoning": { "max_tokens": 2000 } }        // Anthropic/Gemini/Qwen-style budget
{ "reasoning": { "enabled": true, "exclude": true } }
```

- Reasoning tokens are **output tokens**, billed as such, and on most providers **count against `max_tokens`**.
- `usage.completion_tokens_details.reasoning_tokens` reports them. If `completion_tokens - reasoning_tokens ≈ 0`, the model spent the whole budget thinking: you get `finish_reason: "length"`, empty content, and you were still billed. Detect this and either raise `max_tokens` or cap reasoning.
- Some models **cannot disable** reasoning (`reasoning.mandatory: true`), e.g. `openai/gpt-5-mini`, `google/gemini-3.8-flash`, `claude-opus-5.5`. For a grammar pass this is a cost/latency tax — prefer models with `"none"` in `supported_efforts`.
- `reasoning.exclude: true` hides the trace but still bills it.
- Reasoning traces appear as `message.reasoning` (and `reasoning_details` on some paths) — never show raw traces in a writing UI.

Source: [Reasoning tokens](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens).

### 1.12 Rate limits and credit handling

OpenRouter enforces three separate things (this tripped up older integrations that assumed only 429):

1. **Credit limits** — account balance, per-key cap, and an **in-flight spending budget** (estimated cost of running/recently-completed requests held against your balance). A request whose estimate doesn't fit is rejected with **402 before reaching a provider**, even with a positive balance.
2. **Budget limits** — org guardrail budgets, workspace budgets, then the key's own limit; checked in that order; 403.
3. **Rate limits** — free-model RPM/RPD and Cloudflare DDoS protection; 429.

Diagnostics:
- `GET /api/v1/key` returns `limit`, `limit_remaining`, `usage_daily/weekly/monthly`, `is_free_tier`, `free_model_daily_requests`, `budgets[]`, `effective_budget`. Call this before a long batch.
- On 402, branch on `error.metadata.limit_source`:
  - `openrouter_in_flight_budget` → wait for `Retry-After`, retry (transient).
  - `openrouter_key_limit` → key cap exhausted; raise limit or wait for reset.
  - `openrouter_credits` → balance too low, **or** a single request is bigger than the whole budget (`reason: "weight_exceeds_budget"`); retrying won't help.
- Successful inference responses do **not** include `X-RateLimit-*`. Error responses for platform limits do; `Retry-After` appears on 429/503 and the in-flight 402.
- Free-model RPM/RPD numbers are templated in the public docs (rendered per account tier). Don't hardcode them; read `free_model_daily_requests` from `/key`.

Source: [Limits](https://openrouter.ai/docs/api_reference/limits).

### 1.13 Key provisioning (`/keys`) and key limits

- All `/api/v1/keys` routes require a **Management API key** (created at `openrouter.ai/settings/management-keys`), not a normal inference key. Management keys cannot call completions.
- `POST /api/v1/keys` with `{ "name": "…", "limit": 1000 }` returns the key string once; `PATCH /keys/{hash}` supports `disabled`, `limit`, `limit_reset: "daily"|"weekly"|"monthly"`, `include_byok_in_limit`.
- Listing returns `hash` (not the secret), `label`, `limit`, `limit_remaining`, usage windows.

**For this app, provisioning is out of scope.** It's a BYOK desktop app: the user pastes their own key. `/keys` matters only if you later ship a hosted/team tier.

Source: [Management API keys](https://openrouter.ai/docs/guides/overview/auth/management-api-keys).

### 1.14 Can a user-supplied OpenRouter key be validated cheaply?

Yes — and this is the recommended flow:

1. **`GET https://openrouter.ai/api/v1/key`** with `Authorization: Bearer <key>`.
   - `200` → valid; the body tells you `label`, `limit_remaining`, `usage`, `is_free_tier`, and whether free-model quota remains. **No inference, no cost.**
   - `401` → invalid/disabled/expired.
   - This is the single best validation call for OpenRouter: it is free, fast, and returns actionable account state rather than a boolean.
2. **`GET /api/v1/models`** — unauthenticated, so it proves nothing about the key; don't use it for validation, but use it to populate the model picker (and cache it).
3. **A 1-token completion** as a last resort: it proves the key can actually infer, but it costs money, can be blocked by moderation/guardrails, and may fail for model-specific reasons (region, ZDR, deprecated model). Use it only as an explicit "Test connection" action, never on keystroke.

For **other providers**, the equivalent free probe is the models list, which every major provider exposes:

| Provider | Cheap probe |
|---|---|
| OpenAI | `GET https://api.openai.com/v1/models` |
| Anthropic | `GET https://api.anthropic.com/v1/models` (+ `x-api-key`, `anthropic-version`) |
| Google Gemini | `GET https://generativelanguage.googleapis.com/v1beta/models` (+ `x-goog-api-key`) |
| Groq | `GET https://api.groq.com/openai/v1/models` |
| Mistral | `GET https://api.mistral.ai/v1/models` |
| DeepSeek | `GET https://api.deepseek.com/models` |
| xAI | `GET https://api.x.ai/v1/models` |
| Ollama | `GET http://localhost:11434/api/tags` |
| LM Studio | `GET http://localhost:1234/v1/models` |

Standardize on "probe returns 200 and a model list" as the validity contract, with a provider-specific special case for OpenRouter's `/key` (which gives richer state). Cache validation for ~10 minutes; re-validate on 401.

### 1.15 What changed in 2025–2026

- **Multiple API skins**: `/chat/completions`, `/responses`, and `/messages` now coexist. Chat Completions remains the broadest.
- **Routing variants** `:nitro` (throughput), `:floor` (price), `:exacto` (quality-focused provider set), plus `openrouter/auto` and `openrouter/free` routers. Variants are not separate catalog entries.
- **In-region routing** on Business/Enterprise: send to `https://eu.openrouter.ai` or `https://us.openrouter.ai` and prompts/completions stay in-region.
- **Per-model-group ZDR** (Anthropic / OpenAI / Google / xAI / other), replacing a single global toggle; legacy `enforce_zdr` deprecated.
- **Guardrails and org/workspace budgets** with 403s and structured `metadata`.
- **Sticky routing / `session_id`** to maximize cache hits, with a 10-minute idle expiry.
- **Reasoning metadata** in `/models` (`mandatory`, `supported_efforts`, `default_effort`).
- **Response-healing plugin** for JSON repair.
- **Pricing overrides** in `/models` (long-context and time-of-day pricing).

Sources: [Changelog](https://openrouter.ai/docs/changelog), [Models](https://openrouter.ai/docs/guides/overview/models), [ZDR](https://openrouter.ai/docs/guides/features/zdr), [Prompt caching](https://openrouter.ai/docs/guides/best-practices/prompt-caching).

### 1.16 Exact minimal request bodies

**A. Grammar correction (non-streaming, structured).** This is the production shape: stable system prompt + compact schema, document text last.

```json
{
  "model": "anthropic/claude-haiku-5.5",
  "messages": [
    {
      "role": "system",
      "content": "You are a copy editor. Return only minimal, local edits. Never rewrite sentences that are already correct. Quote the exact substring to replace, copied verbatim from the input. If you are unsure, omit the suggestion. Output JSON only."
    },
    {
      "role": "user",
      "content": "<p id=\"p3\">Their going to the conference next week , and they has a presentation.</p>"
    }
  ],
  "temperature": 0,
  "max_tokens": 900,
  "reasoning": { "effort": "none" },
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "grammar_suggestions",
      "strict": true,
      "schema": {
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
                "original": { "type": "string", "description": "Exact substring copied from the paragraph, including punctuation. Must be unique within the paragraph." },
                "replacement": { "type": "string" },
                "category": { "type": "string", "enum": ["grammar", "spelling", "punctuation", "style", "clarity", "conciseness", "word_choice", "consistency"] },
                "severity": { "type": "string", "enum": ["low", "medium", "high"] },
                "explanation": { "type": "string", "description": "One short sentence, max 20 words, naming the rule." }
              }
            }
          }
        }
      }
    }
  },
  "provider": { "require_parameters": true, "data_collection": "deny", "zdr": true },
  "user": "local-install-id"
}
```

**B. Rewrite with structured output (tone / conciseness / humanize).** A rewrite cannot be expressed as a span edit, so the schema returns the full rewritten text plus a change summary, and the client diffs old vs. new for the UI.

```json
{
  "model": "anthropic/claude-haiku-5.5",
  "messages": [
    { "role": "system", "content": "You rewrite text. Preserve meaning, facts, numbers, names, and commitments exactly. Do not add claims. Return JSON only." },
    { "role": "user", "content": "Task: make this email warmer and more concise, target 20% fewer words.\n\n<text id=\"doc1\">Hi team, I am writing to inform you that the deploy is delayed because of a issue in the build pipeline. Please let me know if you have any questions.</text>" }
  ],
  "temperature": 0.2,
  "max_tokens": 1200,
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "rewrite_result",
      "strict": true,
      "schema": {
        "type": "object",
        "additionalProperties": false,
        "required": ["rewritten_text", "changes", "meaning_preserved"],
        "properties": {
          "rewritten_text": { "type": "string" },
          "changes": {
            "type": "array",
            "items": {
              "type": "object",
              "additionalProperties": false,
              "required": ["description", "category"],
              "properties": {
                "description": { "type": "string" },
                "category": { "type": "string", "enum": ["tone", "conciseness", "clarity", "grammar", "flow"] }
              }
            }
          },
          "meaning_preserved": { "type": "boolean", "description": "False if the rewrite required dropping or altering a fact." }
        }
      }
    }
  },
  "provider": { "require_parameters": true }
}
```

> For a **local Ollama/LM Studio** target, drop `provider` and `reasoning`, and keep the same body otherwise (both accept `response_format.json_schema`; see §2.4). For a model without structured outputs, switch to `{"type":"json_object"}` and rely on the repair loop in §3.

---

## 2. Provider abstraction

### 2.1 Strategy

Do **not** model the internal API on OpenAI's schema and then bolt on exceptions. Instead:

1. Define a **canonical internal request/response** that is a superset of what the product needs (messages with typed content parts, tools, structured output, reasoning level, stop, limits).
2. Each provider gets an **adapter** that compiles canonical → wire and parses wire → canonical, plus a **capability descriptor** that says what it can actually do.
3. The **router** picks a model + adapter, validates the request against capabilities *before* sending, and fails with a typed `UnsupportedCapabilityError` rather than silently dropping a parameter.
4. Normalize at the boundary: one `FinishReason` enum, one `Usage` shape, one `ProviderError` taxonomy, one stream event union.
5. Keep the raw provider payload on the response (`raw?: unknown`) for debugging and for `/generation`-style follow-ups.

Why not "just use the OpenAI SDK everywhere": Anthropic requires `max_tokens`, puts the system prompt in a top-level field, has a different streaming protocol and a different structured-output mechanism; Gemini's request is `contents`/`parts` with `generationConfig`; local servers implement a *subset* of OpenAI and silently ignore fields. A thin canonical layer is less code than N special cases scattered through the UI.

### 2.2 Differences that leak through

| Concern | OpenAI Chat Completions | Anthropic Messages | Gemini generateContent | Ollama / LM Studio (OpenAI-compat) |
|---|---|---|---|---|
| **System prompt** | `messages[0].role = "system"` (also `developer` on newer models) | Top-level `system` (string or text-block array); **no system role in messages** | `systemInstruction: {parts:[{text}]}` | OpenAI-style `system` message (supported) |
| **Required fields** | `model`, `messages` | `model`, `max_tokens`, `messages` — **`max_tokens` is mandatory** | `contents` | `model`, `messages` |
| **Max output field** | `max_completion_tokens` (preferred on reasoning models); `max_tokens` deprecated but accepted | `max_tokens` (required) | `generationConfig.maxOutputTokens` | `max_tokens` |
| **Temperature** | `0–2` generally; **reasoning models often only accept the default (1)** | `0–1`; **deprecated on models after Opus 4.6 — only `1.0` accepted, other values 400** | `0–2` | `0–2` (model-dependent) |
| **top_p / top_k** | `top_p`; no `top_k` | `top_p` deprecated alongside temperature on new models; `top_k` exists on some | `topP`, `topK` | `top_p`; `top_k` often ignored |
| **Stop** | `stop` (string or array, up to 4) | `stop_sequences` (array) | `generationConfig.stopSequences` | `stop` (subset) |
| **Structured output** | `response_format: {type:"json_schema", json_schema:{name, strict, schema}}`; strict = constrained decoding; `json_object` legacy | `output_config.format: {type:"json_schema", schema}`; constrained decoding; JSON-schema limitations | `responseMimeType: "application/json"` + `responseSchema` (OpenAPI-ish subset, supports `anyOf`, `enum`, `format`, `minItems`, `prefixItems`) | `response_format` with json_schema (LM Studio: strict supported; Ollama: JSON mode + `response_format`) |
| **Native JSON mode** | `json_object` | n/a (use `output_config`) | `responseMimeType` without schema | `format: "json"` / `response_format` |
| **Tool calling** | `tools` + `tool_choice`; `tool_calls` with JSON-string `arguments` | `tools` with `input_schema`; `tool_use` content blocks; `stop_reason: "tool_use"` | `tools.functionDeclarations`; `functionCall` parts; `toolConfig` | subset; sometimes prompt-templated fallback |
| **Streaming** | SSE `data: {chunk}`, `[DONE]`; usage only if `stream_options.include_usage` | Named SSE events: `message_start`, `content_block_start`, `content_block_delta` (`text_delta` / `input_json_delta` / `thinking_delta`), `content_block_stop`, `message_delta`, `message_stop`, `ping`, `error` | `:streamGenerateContent?alt=sse` (new Interactions API uses step-based events) | OpenAI-style SSE; usage often missing |
| **Token accounting** | `usage.prompt_tokens/completion_tokens`, `prompt_tokens_details.cached_tokens`, `completion_tokens_details.reasoning_tokens` | `usage.input_tokens/output_tokens`, cache read/write counters; `message_delta.usage` is cumulative | `usageMetadata.promptTokenCount/candidatesTokenCount/thoughtsTokenCount/cachedContentTokenCount` | `usage` may be absent or approximate |
| **Finish reason** | `stop`, `length`, `tool_calls`, `content_filter`, `function_call` (legacy) | `end_turn`, `max_tokens`, `stop_sequence`, `tool_use`, `refusal`, `pause_turn` | `STOP`, `MAX_TOKENS`, `SAFETY`, `RECITATION`, `OTHER`, `CONTINUATION` | maps to OpenAI-ish strings, inconsistently |
| **Multimodal** | `content: [{type:"text"|"image_url"|"input_audio"}]` | `content: [{type:"text"|"image"|"document"}]` with base64/URL sources | `parts: [{text}|{inlineData}|{fileData}]` | images supported by some models; check per model |
| **Errors** | `{error:{message,type,param,code}}`; 401 `invalid_api_key`, 429 `rate_limit_exceeded`/`insufficient_quota`, 400 `context_length_exceeded` | `{type:"error", error:{type,message}}`; 400 `invalid_request_error`, 401 `authentication_error`, 403 `permission_error`, 404, 413 `request_too_large`, 429 `rate_limit_error`, 500 `api_error`, **529 `overloaded_error`** | Google error envelope `{error:{code,message,status}}`; `INVALID_ARGUMENT`, `UNAUTHENTICATED`, `PERMISSION_DENIED`, `RESOURCE_EXHAUSTED` (429), `UNAVAILABLE` (503), `DEADLINE_EXCEEDED` (504) | plain HTTP status + `{error:{message}}`; often no type/code |
| **Prefill** | assistant message at end allowed | **removed for Claude 4.6+** (400) | not applicable | model-dependent |
| **Determinism** | `seed` (best effort) | no seed | no seed | `seed` (implementation-defined) |

Anthropic specifics verified in [Messages API create](https://platform.claude.com/docs/en/api/messages/create) and [errors](https://platform.claude.com/docs/en/api/errors). Gemini specifics in [generateContent reference](https://ai.google.dev/api/generate-content) and [structured output](https://ai.google.dev/gemini-api/docs/structured-output). OpenAI in [Structured model outputs](https://developers.openai.com/api/docs/guides/structured-outputs) and [error codes](https://developers.openai.com/api/docs/guides/error-codes).

### 2.3 What this means for adapters

- **Capability gating is mandatory.** A `temperature: 0` on `claude-sonnet-5.5` will 400 (temperature is deprecated and only 1.0 is accepted after Opus 4.6). A `top_k` on GPT is ignored. `max_tokens` is required by Anthropic and optional elsewhere. The canonical request should carry *intent* (`temperature?: number`), and the adapter should drop it when the model's capability descriptor says `supportsTemperature: false` — or better, the router should refuse and pick a compatible model.
- **Reasoning is not portable.** Map canonical `reasoning: "off" | "low" | "medium" | "high" | {budgetTokens}` to `reasoning.effort` (OpenAI/OpenRouter/Gemini), `thinking.budget_tokens` (Anthropic older models) or `thinking: {type:"adaptive"}` (newer), `thinkingConfig.thinkingBudget` (Gemini), `reasoning_effort` (Ollama). On models where reasoning is mandatory, `off` must degrade to the lowest available effort, not error.
- **Structured output is the hardest to unify.** Three mechanisms: OpenAI `json_schema` (strict), Anthropic `output_config.format` (strict/constrained), Gemini `responseSchema` (schema subset). Canonical: `responseFormat: {kind:"json_schema", name, schema, strict:true}`. Adapter maps; the validator validates. Where none is available: `{kind:"json_object"}` + repair loop, or `{kind:"tool", tool}`.
- **Streaming normalizes to one event union**: `start`, `text_delta`, `reasoning_delta`, `tool_call_delta`, `usage`, `done(reason)`, `error`. Anthropic's `content_block_*` and Gemini's chunk array both collapse into this. Do not leak provider event names into the UI.
- **Token accounting normalizes to** `{inputTokens, outputTokens, cachedInputTokens?, cacheWriteTokens?, reasoningTokens?, costUsd?}`. OpenRouter gives `cost` directly; other providers require price lookup. Keep `costUsd` optional and compute from a price table when missing.
- **Errors normalize to** `{kind: "auth"|"rate_limit"|"quota"|"invalid_request"|"context_length"|"content_filter"|"overloaded"|"timeout"|"network"|"server"|"unsupported", httpStatus, providerCode?, retryAfterSeconds?, message, raw}`. The UI switches on `kind`, never on provider strings.

### 2.4 Local servers

- **Ollama**: `http://localhost:11434/v1` OpenAI-compatible (`/v1/chat/completions`), API key is ignored but SDKs require a non-empty value (`"ollama"`). Supports a documented subset including JSON mode, `response_format`, tools, and `reasoning_effort`/`reasoning.effort` mapping. Also exposes an **Anthropic-compatible `/v1/messages`**, which is handy for testing the Anthropic adapter without a key. Native API: `POST /api/chat` with `format` (JSON schema) and `stream`. Source: [Ollama OpenAI compatibility](https://github.com/ollama/ollama/blob/main/docs/api/openai-compatibility.mdx), [Anthropic compatibility](https://github.com/ollama/ollama/blob/main/docs/api/anthropic-compatibility.mdx).
- **LM Studio**: `http://localhost:1234/v1`, `api_key` can be any string (`"lm-studio"`). `response_format: {type:"json_schema", json_schema:{name, strict:"true", schema}}` is supported on `/v1/chat/completions` and follows OpenAI's format. Source: [LM Studio structured output](https://lmstudio.ai/docs/app/api/structured-output).
- Local caveats: no guaranteed `usage`, no `seed` semantics, smaller context, slower first token (model load), and JSON-schema support varies by the loaded model/runtime. Treat local as a first-class provider with its own capability profile, not as "OpenAI with a different URL".

### 2.5 TypeScript interface sketch

```ts
// ─── Identity & capabilities ────────────────────────────────────────────────
export type ProviderId =
  | "openrouter" | "openai" | "anthropic" | "google" | "groq"
  | "mistral" | "deepseek" | "xai" | "ollama" | "lmstudio" | "openai-compatible";

export interface ModelRef { provider: ProviderId; model: string; }

export interface ProviderCapabilities {
  systemPrompt: "message" | "top-level" | "instruction";
  structuredOutput: "json_schema" | "json_object" | "tool" | "none";
  strictSchema: boolean;
  tools: boolean;
  streaming: boolean;
  reasoning: "effort" | "budget" | "both" | "none";
  reasoningMandatory: boolean;
  temperature: { min: number; max: number } | null; // null = do not send
  maxOutputField: "max_tokens" | "max_completion_tokens" | "maxOutputTokens";
  requiresMaxTokens: boolean;
  stopSequences: boolean;
  seed: boolean;
  usageInStream: boolean;
  maxContextTokens: number;
  maxOutputTokens: number;
  inputModalities: Array<"text" | "image" | "file" | "audio">;
}

// ─── Canonical request ──────────────────────────────────────────────────────
export type Role = "system" | "user" | "assistant" | "tool";

export type ContentPart =
  | { type: "text"; text: string }
  | { type: "image"; mimeType: string; dataBase64?: string; url?: string }
  | { type: "file"; mimeType: string; dataBase64?: string; url?: string };

export interface Message {
  role: Role;
  content: string | ContentPart[];
  name?: string;
  toolCallId?: string;
  toolCalls?: ToolCall[];
}

export interface ToolDef {
  name: string;
  description: string;
  parameters: Record<string, unknown>; // JSON Schema
  strict?: boolean;
}

export type ResponseFormat =
  | { kind: "text" }
  | { kind: "json_object" }
  | { kind: "json_schema"; name: string; schema: Record<string, unknown>; strict: boolean };

export type ReasoningRequest =
  | { mode: "off" }
  | { mode: "effort"; effort: "minimal" | "low" | "medium" | "high" | "max" }
  | { mode: "budget"; tokens: number };

export interface ChatRequest {
  model: ModelRef;
  messages: Message[];
  system?: string;                    // canonical; adapter places it correctly
  maxOutputTokens?: number;
  temperature?: number;               // intent; adapter may drop
  topP?: number;
  stop?: string[];
  stream?: boolean;
  responseFormat?: ResponseFormat;
  tools?: ToolDef[];
  toolChoice?: "auto" | "none" | { name: string };
  reasoning?: ReasoningRequest;
  seed?: number;
  /** Routing / provider preferences (OpenRouter maps these; others ignore or error). */
  routing?: {
    order?: string[];
    allowFallbacks?: boolean;
    requireParameters?: boolean;
    dataCollection?: "allow" | "deny";
    zdr?: boolean;
    sort?: "price" | "throughput" | "latency";
    only?: string[];
    ignore?: string[];
  };
  /** Stable id for logs, sticky routing, cache keys. */
  sessionId?: string;
  userId?: string;
  signal?: AbortSignal;
}

// ─── Canonical response ─────────────────────────────────────────────────────
export type FinishReason =
  | "stop" | "length" | "tool_calls" | "content_filter" | "refusal" | "error" | "unknown";

export interface Usage {
  inputTokens: number;
  outputTokens: number;
  cachedInputTokens?: number;
  cacheWriteTokens?: number;
  reasoningTokens?: number;
  costUsd?: number;
  isByok?: boolean;
}

export interface ToolCall {
  id: string;
  name: string;
  argumentsJson: string;
}

export interface ChatResponse {
  id: string;
  model: string;                 // resolved model, may differ from requested
  provider?: string;             // which upstream served it
  text: string;
  reasoning?: string;
  toolCalls: ToolCall[];
  finishReason: FinishReason;
  nativeFinishReason?: string;
  usage: Usage;
  raw?: unknown;
}

// ─── Canonical streaming ────────────────────────────────────────────────────
export type StreamEvent =
  | { type: "start"; model: string; provider?: string }
  | { type: "text_delta"; text: string }
  | { type: "reasoning_delta"; text: string }
  | { type: "tool_call_delta"; index: number; id?: string; name?: string; argumentsDelta?: string }
  | { type: "usage"; usage: Usage }
  | { type: "done"; finishReason: FinishReason; nativeFinishReason?: string }
  | { type: "error"; error: ProviderError };

// ─── Errors ─────────────────────────────────────────────────────────────────
export type ErrorKind =
  | "auth" | "rate_limit" | "quota" | "invalid_request" | "context_length"
  | "content_filter" | "overloaded" | "timeout" | "network" | "server"
  | "unsupported" | "cancelled" | "unknown";

export class ProviderError extends Error {
  constructor(
    public kind: ErrorKind,
    public httpStatus: number | undefined,
    message: string,
    public opts: {
      providerCode?: string;
      retryAfterSeconds?: number;
      requestId?: string;
      retryable?: boolean;
      raw?: unknown;
    } = {},
  ) { super(message); }
}

// ─── Adapter & registry ─────────────────────────────────────────────────────
export interface ProviderAdapter {
  readonly id: ProviderId;
  readonly baseUrl: string;
  capabilities(model: string): Promise<ProviderCapabilities>;
  listModels(): Promise<Array<{ id: string; contextLength?: number; pricing?: unknown }>>;
  validateKey(key: string): Promise<{ ok: boolean; detail?: string }>; // free probe, no inference
  chat(req: ChatRequest, key: string): Promise<ChatResponse>;
  stream(req: ChatRequest, key: string): AsyncIterable<StreamEvent>;
  estimateCost(req: ChatRequest, model: string): Promise<number | undefined>;
}

export interface ProviderRegistry {
  register(adapter: ProviderAdapter): void;
  get(id: ProviderId): ProviderAdapter;
  /** Resolve a model to the best adapter, applying fallbacks. */
  resolve(model: ModelRef): ProviderAdapter;
}
```

**Adapter invariants** (enforce in tests):

1. `chat()` and `stream()` must produce identical `text` for the same request (modulo sampling).
2. `stream()` must always terminate with exactly one `done` or one `error`, even on abort — abort yields `error(kind:"cancelled")` or a clean `done`, never a hang.
3. `usage` is best-effort; missing counts are `undefined`, never `0` (0 is a meaningful value).
4. The adapter never throws raw provider JSON at the UI; everything becomes `ProviderError`.
5. Capability check runs before the network call; unsupported combinations fail fast with `kind:"unsupported"` and a human message.

---

## 3. Structured output for writing tasks

### 3.1 The problem

We need a list of suggestions, each with: the **original span**, a **replacement**, a **category**, an **explanation**, and a **severity** — where the original span must actually exist in the user's document, exactly once, at a known offset, or the UI cannot render a diff or apply the edit.

The hard parts are not JSON syntax. They are:
1. Getting the model to quote spans **verbatim** (it likes to normalize punctuation, smart quotes, spacing).
2. Mapping those quotes back to **UTF-16 offsets** in an `NSTextStorage`/`NSAttributedString` world where emoji and combining marks shift indices.
3. Avoiding **over-editing** (rewriting whole sentences when one word is wrong).
4. **Chunking** long documents without duplicating or losing suggestions at boundaries.
5. **Repairing** malformed or unverifiable output without a second full-price call whenever possible.

### 3.2 Comparing the four mechanisms

| Approach | Schema guarantee | Verifiability of spans | Latency / cost | Portability | Verdict |
|---|---|---|---|---|---|
| **Strict JSON Schema** (`json_schema`, `strict:true`, OpenRouter `require_parameters:true`) | Syntax guaranteed by constrained decoding on capable endpoints; semantics not | Same as any: you still validate the quoted text | 1 call; no retry in the common case | OpenAI, Anthropic, Gemini, OpenRouter, LM Studio support it; older/local models may not | **Primary** |
| **Tool / function calling** | Arguments conform to the tool schema on providers that enforce it; some only "best effort" | Same | 1 call; tool-call parsing overhead | Broad, but OpenRouter falls back to a YAML prompt template for models without native tools — unreliable | **Fallback** when `json_schema` unsupported but `tools` is |
| **JSON mode + retry** (`json_object`) | JSON syntax only, no schema | Same | 1 call + retries on parse/validation failure; retries can double cost | Universal (every OpenAI-compatible server) | **Local / legacy fallback** |
| **Constrained decoding** (llama.cpp GBNF, Outlines) | Strong: invalid tokens masked at sampling time | Same | Local only; grammar compilation and per-token overhead; can distort model quality if grammar is too tight | Only where you control the runtime | **Optional enhancement for local mode** |

Notes:
- OpenRouter's `strict` is **not a uniform guarantee** — some providers translate the schema to their own format or treat it as a hint. Always validate. ([Structured outputs](https://openrouter.ai/docs/guides/features/structured-outputs))
- Anthropic's structured outputs are documented as constrained decoding via `output_config.format`. ([Anthropic structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs))
- Gemini's `responseSchema` is an OpenAPI-style subset and rejects very large/deeply nested schemas; it supports `anyOf`, `enum`, `format`, `minItems`, `prefixItems`. ([Gemini structured output](https://ai.google.dev/gemini-api/docs/structured-output))
- OpenAI strict mode requires `additionalProperties: false` and every property listed in `required`; it supports a subset of JSON Schema (notably **not** `minLength`/`maxLength`/`pattern`/`minimum`/`maximum`/`minItems`/`maxItems` — validate those yourself). ([OpenAI structured outputs](https://developers.openai.com/api/docs/guides/structured-outputs))
- GBNF is a BNF dialect with regex-like extensions, supported by llama.cpp's CLI/server; it can force valid JSON but a too-restrictive grammar can degrade output quality and it does not solve span fidelity. ([llama.cpp GBNF](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md))
- Outlines implements structured generation via FSMs/regex/JSON-schema for local runtimes; same caveat. ([Outlines](https://github.com/dottxt-ai/outlines))

**Recommendation:** strict `json_schema` when the resolved endpoint advertises `structured_outputs`; tool calling when it advertises `tools` but not `structured_outputs`; `json_object` + repair loop for local models and any endpoint that advertises neither. Never rely on prompt-only "return JSON" for production.

### 3.3 Recommended schema (v1)

Keep it flat, small, and free of constraints that strict modes reject. Put length limits in the prompt and enforce them in code.

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
          "paragraph_id": { "type": "string", "description": "id of the <p> block this edit belongs to" },
          "original":     { "type": "string", "description": "verbatim substring of that paragraph; include punctuation; must be unique in the paragraph; max ~120 chars" },
          "replacement":  { "type": "string", "description": "replacement text for exactly that substring; empty string means delete" },
          "category":     { "type": "string", "enum": ["grammar","spelling","punctuation","style","clarity","conciseness","word_choice","consistency"] },
          "severity":     { "type": "string", "enum": ["low","medium","high"] },
          "explanation":  { "type": "string", "description": "one sentence, max 20 words, naming the rule or effect" }
        }
      }
    }
  }
}
```

Design decisions:
- **No offsets from the model.** Models cannot count characters reliably, especially with UTF-16, emoji, or CRLF. Quote + paragraph anchor is the only robust contract.
- **`paragraph_id`** ties the quote to a chunk so the resolver only searches a small string.
- **`original` uniqueness** is required by the prompt and checked in code; if a quote appears twice, resolution is ambiguous and the suggestion is dropped (or disambiguated with a longer quote in the repair pass).
- **`replacement: ""`** encodes deletion without a separate `op` field.
- **`severity`** is categorical, not numeric, so strict enum validation works everywhere.
- **No `confidence`** in v1: models are poorly calibrated, and a fake 0.87 invites bad UI decisions. If you want a confidence signal, derive it from resolution quality (exact vs fuzzy) and model agreement, not from self-report.

### 3.4 Validation and offset mapping

Pipeline for every suggestion:

1. **Schema validation** (ajv or equivalent) → reject/repair.
2. **Locate the paragraph** by `paragraph_id` in the chunk's paragraph map.
3. **Exact match**: `paragraph.indexOf(original)`.
   - 0 matches → try normalization.
   - >1 matches → ambiguous; try anchor disambiguation; else drop.
4. **Normalized match**: NFC-normalize both sides, collapse runs of whitespace, normalize curly quotes/dashes to ASCII, then search. Keep a mapping from normalized indices back to original UTF-16 offsets (build it during normalization; do not re-search the raw string).
5. **Anchor/window match**: use the paragraph + a ±40-char window around the first/last token; compute a diff (`diff-match-patch`) and accept if the matched region's similarity ≥ 0.85 and the edit distance is small relative to length.
6. **Reject** anything still unresolved; record `resolution: "exact" | "normalized" | "fuzzy" | "failed"`.
7. **Post-checks**: replacement ≠ original; span length ≤ 2× the original or ≤ 200 chars; no overlap with a higher-severity accepted suggestion; category ∈ enum; explanation ≤ 25 words.
8. **Offsets**: store `{ utf16Start, utf16End, graphemeStart, graphemeEnd }`.
   - Swift `String.Index` is a grapheme-cluster index; `NSRange` for `NSTextStorage`/TextKit is **UTF-16**. Convert with `String.utf16.distance(from:to:)`.
   - Never assume `original.count == original.utf16.count`; emoji and flags break that.
   - Normalization can change length (e.g. `é` as `e` + combining accent vs. precomposed) — the index map must be built, not approximated.

### 3.5 Repair loop

Budget: at most **2 extra calls**, and only when the first call is structurally or semantically unusable.

```
call(model, request)
  → parse JSON (jsonrepair if needed)
  → schema validate
      fail → REPAIR_1: same model, add the validator error + "return only JSON matching the schema",
              temperature 0, max_tokens bumped 1.5×
  → resolve spans
      resolutionRate = resolved / total
      if resolutionRate < 0.8 and unresolved.length > 0:
          REPAIR_2: send only the unresolved originals + the exact paragraph text,
                    ask for corrected `original` quotes (cheap, small prompt)
  → still failing:
      switch mechanism (json_schema → tool call, or json_object)
      then switch model (cheap → mid tier)
  → final: drop unresolved, surface "N suggestions could not be located" in the UI
```

Triggers for escalation to a better model (not just repair):
- schema failure twice,
- resolution rate < 0.6,
- zero suggestions on a paragraph with an obvious error (only detectable with a heuristic or a second opinion),
- `finish_reason: "length"` with `reasoning_tokens ≈ completion_tokens`,
- user taps "looks wrong" on a review.

Never loop more than 3 total attempts per chunk; cap spend per document.

### 3.6 Idempotence

- `temperature: 0` and a fixed `seed` where supported; still not guaranteed (Anthropic documents non-determinism even at temperature 0).
- Cache key = `SHA256(normalizedText || model || promptVersion || schemaVersion || paramsHash)`. Same key → same stored result, no network call. See §7.4.
- Store `promptVersion` and `schemaVersion` in every result row so you can invalidate cleanly.
- The resolver must be deterministic: same input JSON → same accepted set.

### 3.7 Long documents: chunking, stable IDs, dedupe

**Chunking**
- Split on paragraph boundaries first (`\n\n`), then sentence boundaries inside oversized paragraphs, never mid-word and never mid-sentence if avoidable.
- Target **600–1,200 tokens** per chunk (roughly 450–900 words), hard cap at ~2,000 tokens or 25% of the model's context, whichever is smaller.
- Overlap by **one sentence** at each boundary so an error spanning a boundary is seen whole.
- Keep a `ChunkMap`: `chunkId → [{ paragraphId, globalUtf16Range }]`.
- Stable IDs: `c{chunkIndex}` / `p{paragraphIndex}` / `s{sentenceIndex}` derived from the **original document**, not from chunk order. If the user edits paragraph 3, its ID doesn't change; only the chunk containing it is re-run. This is what makes incremental re-review cheap.
- Re-review only dirty chunks: hash each paragraph, compare to stored hashes.

**Deduplication**
- Normalize suggestions to `(globalStart, globalEnd, replacement)`.
- Exact duplicates (same span + same replacement) → keep one, take the higher severity, merge explanations.
- Overlapping spans:
  - identical replacement → merge into the union span (if the union is still a valid substring).
  - different replacements → keep the one with higher severity, then higher resolution quality (exact > normalized > fuzzy), then the one from the chunk where the span is fully interior (not clipped by the overlap window).
- Boundary duplicates from overlap: prefer the suggestion whose span is entirely within one chunk; drop the copy from the overlapped region.
- Cap total suggestions per document (e.g. 100) and per paragraph (e.g. 8) to keep the UI usable; sort by severity then document order.

**Cost control**: chunk-level calls can run in parallel (3–4 concurrent) with a global concurrency limit; each chunk is independently retryable, so a single failure doesn't lose the document.

---

## 4. Prompting and quality

### 4.1 System prompt (grammar/style)

Principles that matter more than clever phrasing:
- **Minimal edit policy**: "Change the fewest characters necessary. If a sentence is already correct, return nothing for it."
- **Verbatim quoting**: "Copy the original text exactly, including punctuation and spacing. Do not fix quotes or dashes in the `original` field."
- **No full rewrites** in grammar mode: "Do not rewrite for style unless the category is style/clarity."
- **Omit when unsure**: "If you are less than 80% confident, omit the suggestion." (Better to miss than to hallucinate a span.)
- **Explanation budget**: "At most 20 words, name the rule (e.g. 'subject–verb agreement'), no praise, no restating the sentence."
- **No meta-commentary**: "Return JSON only. No markdown fences, no preamble."
- **Preserve meaning**: "Never change names, numbers, dates, or commitments."

Template:

```
You are a precise copy editor inside a writing app.
Rules:
1. Suggest only local edits: a word, phrase, or punctuation mark.
2. `original` must be copied verbatim from the paragraph, including punctuation.
3. Do not suggest an edit if the text is already correct or the improvement is subjective.
4. Prefer the smallest change that fixes the problem. Never rewrite a whole sentence for grammar.
5. One suggestion per problem. Do not stack overlapping edits.
6. `explanation` ≤ 20 words, names the rule, no praise.
7. If unsure, omit. Missing an edit is better than a wrong one.
8. Output JSON only.
```

User message:

```
<paragraph id="p3">
Their going to the conference next week , and they has a presentation.
</paragraph>
```

### 4.2 Task variants

**Tone rewrite.** Separate task, full-text output. Constraints: target tone (warm/professional/direct/apologetic), preserve facts/numbers/names, keep length within ±15%, keep the greeting/sign-off unless told otherwise, no new promises. Return `rewritten_text` + `changes[]` + `meaning_preserved` (schema in §1.16B). Ask for the rewrite first and the change list second in the prompt; the schema order doesn't control generation order, so put the important field first in `required`.

**Conciseness.** Give a target ("reduce by ~20%") and hard rules: keep all facts, keep all names/dates, keep the call to action, prefer deleting hedges/filler ("I just wanted to", "in order to" → "to"). Cap the reduction (e.g. no more than 40%) to prevent content loss.

**Humanize.** Highest risk of meaning drift. Constraints:
- keep every entity, number, and commitment;
- sentence count within ±20%;
- no new claims, no added anecdotes;
- banned list: "delve", "tapestry", "in today's fast-paced world", "it's important to note";
- vary sentence length but don't split/merge across paragraph boundaries;
- output a `meaning_preserved` flag and a diff summary;
- run an automatic invariant check (all numbers and capitalized entities from the source must appear in the output) and reject/regenerate on failure.

### 4.3 Keeping explanations short

- Prompt constraint ("≤ 20 words") is necessary but not sufficient; models drift.
- Post-process: truncate at the first sentence; strip lead-ins ("This is because", "The reason is"); map categories to short templates:
  - grammar → "Subject–verb agreement: *they has* → *they have*."
  - spelling → "Misspelling of *their*."
  - punctuation → "Missing comma before the conjunction."
- Store both the model's explanation and the rendered short form; show the short form by default.

### 4.4 Avoiding meaning changes

Layered defense:
1. Prompt constraints + `meaning_preserved` boolean.
2. **Invariant check**: every number, date, proper noun, and email/URL in the source must appear in the rewrite. If not, reject.
3. **Length guard**: reject rewrites outside ±40% of the source length (tone tasks) or outside the requested reduction band (conciseness).
4. **Diff review**: the UI shows a word-level diff; the user accepts per-hunk, never a silent full replacement.
5. Optional **NLI/entailment check**: a small local model or a cheap API call asking "does the rewrite entail the original and vice versa?" — only for high-stakes documents, since it doubles cost.

### 4.5 Evaluation

Build a **golden set** of 200–500 items, stratified by:
- error type (grammar, spelling, punctuation, style, clarity, tone, conciseness),
- document length (email, paragraph, 1,500-word doc),
- text source (native, learner, non-native, domain jargon),
- difficulty (obvious vs. subtle vs. no-error control).

Metrics:

| Metric | What it measures | How |
|---|---|---|
| **Span precision / recall / F0.5** | Did we find the right edits? | Compare to gold spans; F0.5 weights precision (matches CoNLL-2014's official scorer convention) |
| **Exact acceptance rate** | Would a user accept without further editing? | Human or heuristic: replacement equals gold replacement |
| **ERRANT M2 F0.5** | Standard GEC score, plus per-category breakdown | Run `errant` on hypothesis/reference pairs |
| **GLEU** | Sentence-level fluency/correctness correlation | Standard GLEU implementation |
| **Meaning preservation** | Numbers/entities retained; no new claims | Invariant check + spot-check |
| **Over-edit rate** | Edits on already-correct text | No-error control items; count suggestions |
| **Schema/parse failure rate** | Reliability | Telemetry per model |
| **Resolution rate** | Fraction of returned spans locatable in the source | Client-side |
| **Latency p50/p95, cost per review** | UX/economics | Telemetry |
| **LLM-as-judge rubric** | Holistic quality (1–5) on correctness, minimality, explanation quality | Pairwise comparisons, randomized order, calibrated on ~100 human labels |

**LLM-as-judge cautions**: judges are biased toward verbosity, early positions, and their own outputs. Use pairwise (A/B) rather than absolute scores, randomize order, hide model identity, and report agreement with human labels (Cohen's κ). Use the judge for *relative* model selection, not as the only gate.

**Datasets for regression tests** (use as evaluation only; check licenses before shipping fixtures):
- **BEA-2019 Shared Task** — Write&Improve+LOCNESS corpus, wider range of native/learner levels. [ACL Anthology W19-4406](https://aclanthology.org/W19-4406/)
- **CoNLL-2014 Shared Task** — 50 test essays, official M2 scorer and F0.5 convention. [ACL Anthology W14-1701](https://aclanthology.org/W14-1701/)
- **JFLEG** — fluency-oriented holistic edits, useful for rewrite/humanize evaluation. [ACL Anthology E17-2037](https://aclanthology.org/E17-2037/)
- **ERRANT** — error-type annotation toolkit and M2 scorer; the standard for per-category GEC evaluation. [github.com/chrisjbryant/errant](https://github.com/chrisjbryant/errant) (Bryant, Felice & Briscoe, ACL 2017, [P17-1074](https://aclanthology.org/P17-1074/))
- **GLEU** — sentence-level metric introduced by Napoles et al. (2015/2016); implemented in several open toolkits. Useful as a secondary, less brittle signal than M2 F0.5.

Practical regression gate in CI: run 50–100 golden items on every prompt/model change; fail if span F0.5 drops >2 points or resolution rate drops >5 points or cost/review rises >20%.

---

## 5. Cost and latency

### 5.1 Representative models (live catalog, 2026-10-19)

Prices are USD per **1M tokens**, model-level, from `GET /api/v1/models`. Endpoint-level prices can be lower (see §1.7) — e.g. `gpt-5-mini` has an endpoint at $0.125/$1.00.

| Tier | Model | Input $/M | Output $/M | Context | Max out | Structured | Reasoning | Notes |
|---|---|---|---|---|---|---|---|---|
| Ultra-cheap | `deepseek/deepseek-v4-flash` | 0.091 | 0.182 | 1.05M | 943k | ✅ | optional (`none` available) | Cheapest credible option; text-only |
| Cheap | `anthropic/claude-haiku-5.5` | 0.10 | 0.50 | 1M | 128k | ✅ | optional (`none`) | **Best default**: cheap, fast, 1M ctx, multimodal, long-ctx surcharge >100k |
| Cheap | `google/gemini-3.1-flash-lite` | 0.25 | 1.50 | 1.05M | 65k | ✅ | optional (`none`) | Multimodal (image/file/audio/video), 65k output cap |
| Cheap | `openai/gpt-5-mini` | 0.25 | 2.00 | 400k | 128k | ✅ | **mandatory** | Quality strong, but pays a reasoning tax on every call |
| Mid-cheap | `google/gemini-3.8-flash` | 0.825 | 4.125 | 1.05M | 65k | ✅ | **mandatory** | Good quality, reasoning always on |
| Mid | `anthropic/claude-sonnet-5` | 2.00 | 10.00 | 1M | 128k | ✅ | optional | Strong writing quality; escalation target |
| Mid | `openai/gpt-5.4` | 2.50 | 15.00 | 1.05M | 128k | ✅ | optional (`none`) | Long-ctx surcharge >272k |
| Frontier | `openai/gpt-5.6-sol` | 4.00 | 20.00 | 1.05M | 128k | ✅ | optional | Frontier quality; use sparingly |
| Frontier | `anthropic/claude-opus-5.5` | 4.00 | 20.00 | 1M | 128k | ✅ | **mandatory** | Highest writing quality tier we'd ship |
| Ultra | `openai/gpt-5.5-pro` | 30.00 | 180.00 | 1.05M | 128k | ✅ | mandatory | Not for interactive review |

### 5.2 Cost model and assumptions

Token assumptions (English, ~1.33 tokens/word):
- **150-word email**: ~200 input text tokens + ~400 tokens of system prompt + schema + instructions = **600 input**; output ~**450** tokens (≈8 suggestions × ~55 tokens).
- **1,500-word document**: ~2,000 input text tokens, chunked into ~4 calls with ~400 tokens of per-call overhead = **3,600 input**; output ~**1,400** tokens (≈25 suggestions).
- No cache, no retries.

| Model | Email $/review | Email $/1k | Doc $/review | Doc $/1k |
|---|---:|---:|---:|---:|
| deepseek-v4-flash | 0.00014 | **$0.14** | 0.00058 | **$0.58** |
| claude-haiku-5.5 | 0.00028 | **$0.28** | 0.00106 | **$1.06** |
| gemini-3.1-flash-lite | 0.00082 | $0.82 | 0.00300 | $3.00 |
| gpt-5-mini | 0.00105 | $1.05 | 0.00370 | $3.70 |
| gemini-3.8-flash | 0.00235 | $2.35 | 0.00874 | $8.74 |
| claude-sonnet-5 | 0.00570 | $5.70 | 0.02120 | $21.20 |
| gpt-5.4 | 0.00825 | $8.25 | 0.03000 | $30.00 |
| gpt-5.6-sol | 0.01140 | $11.40 | 0.04240 | $42.40 |
| claude-opus-5.5 | 0.01140 | $11.40 | 0.04240 | $42.40 |
| gpt-5.5-pro | 0.09900 | $99.00 | 0.36000 | $360.00 |

**Blended cheap-first / retry-with-better** (85% resolved by Haiku 5.5, 15% escalate to Sonnet 5 including the wasted cheap call):
- Email: `0.85×0.00028 + 0.15×(0.00028+0.00570)` = **$0.00114/review → $1.14 per 1,000**
- Document: `0.85×0.00106 + 0.15×(0.00106+0.02120)` = **$0.00424/review → $4.24 per 1,000**

If 10% of documents are escalated all the way to a frontier model, add roughly $0.4–$1.0 per 1,000 documents — still negligible relative to UX gains.

### 5.3 Latency

No public per-model latency figures were available from the catalog (`stats` was `null` on every endpoint queried). The estimates below are modelled from `TTFT + output_tokens / throughput` and should be replaced with your own telemetry after launch. **[estimate]**

Assumptions: cheap models 150–250 tok/s output, 0.3–0.8 s TTFT; mid 80–150 tok/s, 0.5–1.2 s TTFT; frontier reasoning 40–80 tok/s + 1–5 s thinking.

| Scenario | Model tier | Estimated wall clock |
|---|---|---|
| 150-word email, 8 suggestions, non-streaming | cheap (Haiku 5.5, reasoning off) | **2–4 s** |
| 150-word email, streaming first suggestion | cheap | **0.8–1.8 s** to first rendered suggestion |
| 150-word email | mid (Sonnet 5) | 4–7 s |
| 1,500-word doc, 4 chunks in parallel | cheap | **4–8 s** (parallelism hides most of it) |
| 1,500-word doc | frontier with reasoning | 15–40 s |
| Local Ollama 8B, cold model load | local | 10–30 s first call, 3–8 s warm |

UX recommendation: stream the **first chunk's suggestions** rather than waiting for the whole document; show per-paragraph progress; never block typing on a network call.

### 5.4 Recommended default models and strategy

1. **Default**: `anthropic/claude-haiku-5.5`, `reasoning.effort: "none"`, `temperature: 0`, structured output.
   - Why: cheapest capable model ($0.10/$0.50), 1M context, multimodal, structured outputs, and reasoning can be disabled — unlike `gpt-5-mini` and `gemini-3.8-flash`, where reasoning is mandatory and inflates cost/latency.
2. **Fast alternate**: `google/gemini-3.1-flash-lite` for users who prefer Google, or when Haiku is unavailable under ZDR/routing constraints.
3. **Escalation**: `anthropic/claude-sonnet-5`, then `openai/gpt-5.6-sol` (or `claude-opus-5.5`) for "deep review" / "improve my writing" actions.
4. **Budget option**: `deepseek/deepseek-v4-flash` — extremely cheap, but text-only and quality-sensitive; offer it as an explicit "cheapest" choice, not the default.
5. **Local**: Ollama + a 7–14B instruct model for offline/privacy mode; accept lower quality and no guaranteed JSON schema.

**Cheap-first/retry logic** (pseudo):

```
try cheap(model=haiku, effort=none, timeout=12s)
  accept if: schema valid, resolutionRate ≥ 0.8, no length-truncation,
             and (suggestions > 0 or document has no error signals)
  else escalate
try mid(model=sonnet, effort=low, timeout=25s)
  accept on same criteria
else surface partial results + "retry with a stronger model" button
```

Never auto-escalate on **user-visible** latency alone (that hides cost); escalate on **quality signals**. Cap escalations per document (e.g. 2) and show the cost delta in settings.

### 5.5 Prompt caching

- **OpenRouter**: uses **provider sticky routing** to maximize cache hits; sticky sessions expire after 10 minutes of inactivity; pass `session_id` (or `x-session-id`, or `prompt_cache_key`) to pin. Sticky routing is disabled when you set `provider.order`. ([Prompt caching](https://openrouter.ai/docs/guides/best-practices/prompt-caching))
- **Anthropic**: explicit `cache_control` breakpoints, 5-minute default TTL, `"ttl": "1h"` for 2× write cost; minimum cacheable prefix is 512 tokens on Claude 5-family models (4,096 on older ones). Cache reads are ~0.1× input; writes ~1.25× (5m) or 2× (1h).
- **OpenAI**: automatic caching on prompts ≥1,024 tokens; reads at 0.25×–0.5× input. **GPT-5.6 and later charge cache writes at 1.25× input even for automatic caching**, so a one-off long prompt costs *more* with caching on; you can disable it with `prompt_cache_options: {mode:"explicit"}` and no breakpoints.
- **Gemini**: implicit caching; `cachedContentTokenCount` in `usageMetadata`.
- **Our prompt layout**: stable prefix first (system prompt + schema + few-shot examples), variable content last (document chunk). Keep the prefix ≥1,024 tokens if you want OpenAI caching to engage; otherwise caching buys little for short email prompts. For long documents, the per-chunk overhead is repeated 4–8×, so caching the prefix is worth roughly 5–15% of total cost.
- **Batch**: OpenRouter exposes `:batch` model variants at roughly 50% of interactive price (e.g. `claude-haiku-5.5:batch` $0.05/$0.25). Use for offline evaluation and bulk re-processing, never for interactive review.

---

## 6. Privacy and security

### 6.1 Storing API keys in the macOS Keychain

Recommended:

```swift
let query: [String: Any] = [
    kSecClass as String: kSecClassGenericPassword,
    kSecAttrService as String: "com.example.Grammerly.providers",
    kSecAttrAccount as String: "openrouter",              // provider id
    kSecAttrSynchronizable as String: false,              // never iCloud Keychain
    kSecUseDataProtectionKeychain as String: true,        // macOS: use the iOS-style keychain
    kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
    kSecAttrAccessControl as String: accessControl,
    kSecValueData as String: secretData
]

let accessControl = SecAccessControlCreateWithFlags(
    nil,
    kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
    [.userPresence],            // Touch ID / Watch / password fallback
    &error
)
```

Why each piece:
- **`kSecUseDataProtectionKeychain: true`** — on macOS, the legacy keychain APIs default to the *login keychain* (file-based), where items can be synced by iCloud Keychain and where `kSecAttrAccessible*` semantics don't apply the same way. The Data Protection keychain gives iOS-equivalent behaviour, including access control and `ThisDeviceOnly`. Apple documents the platform difference in [TN3137: On Mac keychain APIs and implementations](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains); the key is documented at [kSecUseDataProtectionKeychain](https://developer.apple.com/documentation/security/ksecusedataprotectionkeychain).
- **`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`** — readable only while the device is unlocked; **not** migrated to a new device and **not** included in backups. ([Apple docs](https://developer.apple.com/documentation/security/ksecattraccessiblewhenunlockedthisdeviceonly))
- **`kSecAttrSynchronizable: false`** — explicit opt-out of iCloud Keychain sync. Default is false, but set it explicitly so a future default change or a copied query can't silently sync.
- **`SecAccessControl` with `.userPresence`** — requires Touch ID or device passcode to read; `.biometryCurrentSet` invalidates the item if the enrolled fingerprints/face change (good for high-security mode, annoying for daily use). Offer `.userPresence` by default and `.biometryCurrentSet` as an option.
- **Access group** (`kSecAttrAccessGroup`) — only if you have app extensions (menu-bar helper, share extension) that need the same key; otherwise leave unset.
- **Never** store keys in `UserDefaults`, plists, environment files, or the app bundle. Never log them; redact `Authorization` headers in crash/network logs. Use a `SecureBytes` wrapper and zero buffers where practical.
- **Key rotation/removal**: `SecItemDelete` on sign-out; re-validate on 401; show the key's last 4 characters (not the whole key) in settings.

### 6.2 Redaction of PII before sending

Redaction is **opt-in and per-document**, because it degrades grammar quality (replacing a name with `[PERSON_1]` breaks agreement cues and can produce wrong suggestions).

Layered approach:
1. **Regex pass** (cheap, deterministic): emails, phone numbers, credit-card-like digit runs (Luhn check), IBAN, SSN/national IDs, URLs with tokens, IP addresses, API-key-shaped strings (`sk-…`, `ghp_…`).
2. **NER pass** (optional, local): names, addresses, organizations — use a small on-device model or a local spaCy/Presidio-style pipeline; never send text to a third-party redaction service.
3. **Placeholder map**: `{{EMAIL_1}}` → original, kept only in memory for the session; restore in suggestions before display.
4. **Warn**: "Redaction is on. Suggestions may be less accurate for sentences containing redacted names."
5. **Local-only alternative**: if the user picks Ollama/LM Studio, skip redaction by default — the text never leaves the machine.

Never claim redaction is perfect. Label it "best-effort PII masking".

### 6.3 Per-app data policies and OpenRouter controls

Expose a single "Privacy" toggle that maps to OpenRouter request fields:

```json
"provider": {
  "data_collection": "deny",
  "zdr": true,
  "require_parameters": true
}
```

- `data_collection: "deny"` restricts routing to providers that do not store data non-transiently and do not train on it. Default is `"allow"`. ([Provider routing](https://openrouter.ai/docs/guides/routing/provider-selection#requiring-providers-to-comply-with-data-policies))
- `zdr: true` restricts to Zero Data Retention endpoints. It **ORs** with account-wide and guardrail ZDR — you can turn it on per request but cannot use it to override an account that already enforces ZDR. ZDR does **not** cover plugins/tools (web search etc.), which have their own policies. ([ZDR](https://openrouter.ai/docs/guides/features/zdr))
- ZDR can also be enforced per model group (Anthropic/OpenAI/Google/xAI/other) in account settings; the request-level flag is the portable way to do it from a desktop app.
- Show the **served provider** in the UI ("Served by Anthropic, zero-retention") using routing metadata or `/generation`; it builds trust and catches misconfiguration.
- `provider.only: ["anthropic"]` is the hard allowlist if the user wants a specific vendor.

**Retention facts to surface (verify wording before shipping):**
- OpenRouter itself **does not store prompts or responses** unless the user opts in to private logging or to "use of inputs/outputs" (which gives a 1% discount). It does store request metadata (token counts, latency). ([Data collection](https://openrouter.ai/docs/guides/privacy/data-collection))
- Provider retention/training policies vary per endpoint; OpenRouter tags them and exposes a ZDR endpoint list at `GET /api/v1/endpoints/zdr`. ([Provider logging](https://openrouter.ai/docs/guides/privacy/provider-logging))
- OpenAI's API does not train on customer data by default and retains abuse-monitoring logs for up to 30 days; Anthropic's commercial API does not train on inputs/outputs by default. **[verify before shipping]** — these are policy claims that change; link to the providers' current policies rather than hardcoding text in the app.

### 6.4 Local-only mode

- Provider = Ollama (`http://localhost:11434/v1`) or LM Studio (`http://localhost:1234/v1`); no outbound network for inference.
- Detect running servers; if none, show setup instructions and a "Download a model" link; never silently fall back to a cloud provider.
- Model recommendation for grammar: a 7–14B instruct model with good JSON adherence; test candidates against the golden set before recommending.
- Disable analytics/telemetry in this mode; the app should be fully functional offline after model download.
- Verify: an in-app network log (host + timestamp) and a unit test that asserts no `URLSession` task is created in local mode.

### 6.5 Making "your text never touches our servers" verifiable

You can make a strong, honest claim because the app is open source and BYOK:

1. **No first-party backend for inference.** The app talks directly to the provider's host; there is no proxy you operate. State this precisely: "We don't operate a server that receives your text. Your text goes from your Mac to the provider you chose."
2. **Open source + reproducible builds.** Publish the exact source, the build script, and a notarized release; document how to build from source and compare hashes.
3. **Network audit surface.** A settings screen listing every host the app can contact (provider hosts, update check, optional telemetry), plus a debug log of actual requests. Add a test that fails if a new host is introduced without updating the allowlist.
4. **No telemetry by default.** If you add crash reporting, make it opt-in, scrub text, and document exactly what is sent.
5. **Entitlements review.** Avoid the `com.apple.security.network.client` sandbox entitlement being used for anything beyond provider calls; document it. If sandboxed, list the exact outgoing connections.
6. **Provider transparency.** Show which provider served each request and link to its retention policy; offer `zdr`/`deny` toggles that are on by default for privacy mode.
7. **Avoid overclaiming.** "Never touches our servers" is true only for the inference path — update checks, license validation, and crash reports are still network calls. Say "no first-party inference backend; here is the complete list of network calls."

---

## 7. Rate limits, retries, offline

### 7.1 Retry and backoff

Classify before retrying (see `ProviderError.kind`):

| Kind | Retry? | Policy |
|---|---|---|
| `auth` (401) | no | Prompt to re-enter key; re-validate |
| `quota` (402/403) | no (except in-flight 402) | Show balance/top-up; for in-flight 402 honor `Retry-After` and retry once |
| `invalid_request` (400/404/413/422) | no | Bug or unsupported model; log and surface |
| `context_length` (400) | no | Chunk smaller / raise model |
| `content_filter` (400/403) | no | Show which text was flagged (if provided) |
| `rate_limit` (429) | yes | Honor `Retry-After`; else exponential backoff with jitter |
| `overloaded` (503/529) | yes | Backoff; consider fallback provider/model |
| `timeout`/`network` | yes | Backoff; then offline mode |
| `server` (500/502) | yes | Backoff; fallback provider |
| `cancelled` | no | User aborted |

Backoff: base 500 ms, multiplier 2, full jitter, cap 8 s, max 3 attempts (4 for streaming reconnects). Honor `Retry-After` exactly (seconds or HTTP-date). Add a per-provider **circuit breaker**: after 5 consecutive failures, open for 60 s, then half-open with one probe.

Timeouts: connect 10 s; first byte 30 s (cloud) / 120 s (local, model load); idle stream 60 s; total 120 s cloud / 300 s local. Use `URLSessionConfiguration.timeoutIntervalForRequest` plus a watchdog for first-byte.

### 7.2 Cancellation (streaming abort)

- Hold the `Task`/`URLSessionDataTask` and the `AsyncThrowingStream` continuation; on cancel call `task.cancel()` and `continuation.finish(throwing: CancellationError())`.
- For `URLSession.bytes(for:)`, break out of the `for try await` loop and cancel the task; the connection closes.
- The adapter's `stream()` must treat cancellation as a terminal `error(kind:"cancelled")` (or a clean `done` with `finishReason:"stop"` plus a `cancelled` flag) and must not retry.
- On the server side, cancellation stops billing for tokens not generated; already-generated tokens are still charged. Don't cancel-and-retry on user edits without a cache key change.

### 7.3 Surfacing 429 / 402 / 401

| Status | User-facing message | Action |
|---|---|---|
| **401** | "Your OpenRouter key was rejected. It may be revoked or expired." | "Update key" button → settings; re-validate with `/key` |
| **402 (key limit)** | "You've hit the spending limit on this API key." | Show `limit_remaining`, link to OpenRouter key settings |
| **402 (credits)** | "Your OpenRouter account is out of credits." | Link to top-up; offer local model |
| **402 (in-flight)** | "Too many requests in flight — retrying in Ns." | Auto-retry after `Retry-After` |
| **403 (budget/guardrail)** | "Your organization's budget or guardrail blocked this request." | Show `metadata.remedy_hint`; don't retry |
| **429** | "Rate limited by the provider. Retrying…" | Auto-retry with backoff; show attempt count after 2 failures |
| **503** | "No provider is available for this model right now." | Offer a different model / retry |
| **Network** | "You're offline. Your work is saved locally." | Queue; offer local model |

Never show raw provider JSON. Log it with a request id; show the human sentence plus a "Details" disclosure.

### 7.4 Caching design

Key: `SHA256( normalize(text) + "\u0000" + modelId + "\u0000" + promptVersion + "\u0000" + schemaVersion + "\u0000" + paramsHash )`.

- Normalize: NFC, collapse whitespace, strip trailing spaces, keep case and punctuation (they matter).
- `paramsHash` covers temperature, max tokens, reasoning effort, response format.
- Store: key, result JSON, model, prompt/schema version, created_at, latency_ms, cost_usd, token counts.
- TTL: 30 days for review results; invalidate on prompt/schema version bump; per-document edits invalidate only that document's chunks (paragraph hashes).
- Privacy: store the hash and the **result**, not the full source text, unless the user enables history. If you store text, encrypt at rest with a Keychain-held key (CryptoKit `AES.GCM`), and exclude from backups (`URLResourceValues.isExcludedFromBackup`).
- Negative caching: cache "no suggestions" too, but with a shorter TTL and a flag so a model change re-runs.

### 7.5 Local persistence (SQLite/GRDB)

Use **GRDB** (Swift SQLite toolkit) with WAL mode and migrations.

```sql
-- documents and their text (optional, user-controlled history)
CREATE TABLE document (
  id TEXT PRIMARY KEY,
  title TEXT,
  text TEXT,
  created_at REAL NOT NULL,
  updated_at REAL NOT NULL,
  content_hash TEXT NOT NULL
);

-- paragraph-level hashes for incremental re-review
CREATE TABLE paragraph (
  id TEXT PRIMARY KEY,              -- stable p{index}
  document_id TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  text_hash TEXT NOT NULL,
  utf16_start INTEGER NOT NULL,
  utf16_end INTEGER NOT NULL
);

-- one review run
CREATE TABLE review (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  prompt_version TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  created_at REAL NOT NULL,
  input_tokens INTEGER, output_tokens INTEGER, cost_usd REAL,
  latency_ms INTEGER,
  status TEXT NOT NULL              -- ok | partial | failed
);

-- suggestions
CREATE TABLE suggestion (
  id TEXT PRIMARY KEY,
  review_id TEXT NOT NULL REFERENCES review(id) ON DELETE CASCADE,
  paragraph_id TEXT NOT NULL,
  utf16_start INTEGER NOT NULL,
  utf16_end INTEGER NOT NULL,
  original TEXT NOT NULL,
  replacement TEXT NOT NULL,
  category TEXT NOT NULL,
  severity TEXT NOT NULL,
  explanation TEXT,
  resolution TEXT NOT NULL,         -- exact | normalized | fuzzy
  accepted INTEGER                     -- NULL = undecided, 0/1
);

-- response cache
CREATE TABLE review_cache (
  cache_key TEXT PRIMARY KEY,
  result_json TEXT NOT NULL,
  model TEXT NOT NULL,
  prompt_version TEXT NOT NULL,
  created_at REAL NOT NULL,
  expires_at REAL
);

-- user dictionary / ignore rules
CREATE TABLE dictionary_entry (
  id TEXT PRIMARY KEY,
  term TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL,               -- allow | ignore_rule | replacement
  payload TEXT,
  created_at REAL NOT NULL
);

-- request log for transparency/debugging (no prompt text by default)
CREATE TABLE request_log (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  host TEXT NOT NULL,
  status INTEGER,
  error_kind TEXT,
  latency_ms INTEGER,
  created_at REAL NOT NULL
);

CREATE INDEX idx_suggestion_review ON suggestion(review_id);
CREATE INDEX idx_paragraph_document ON paragraph(document_id);
CREATE INDEX idx_cache_expiry ON review_cache(expires_at);
```

Use FTS5 (`dictionary_entry_fts`) if you want fuzzy dictionary search. Keep the DB in `Application Support`, exclude it from iCloud backup if it contains document text, and offer "Clear history" / "Clear cache" separately.

### 7.6 Offline behaviour

- All UI works offline; reviews queue and run when connectivity returns (or immediately on a local model).
- Detect offline with `NWPathMonitor`; don't retry cloud calls while offline — switch to a "queued" state.
- If the user has a local model, offer "Review offline with local model" as a first-class action.
- Persist the queued job (document id + chunk ids + model) so it survives app restart.
- Never lose edits: the document buffer is local; network is an enhancement, not a dependency.

---

## 8. Open questions / follow-ups

1. **Per-provider JSON Schema keyword support** — the exact supported/unsupported keyword table for OpenAI strict, Anthropic, Gemini, and each OpenRouter endpoint should be encoded in capability descriptors and tested. This pass verified the broad shape, not every keyword. **[verify before shipping]**
2. **Latency** — no public per-endpoint latency stats were available (`stats: null`). Instrument TTFT and total time per model/provider from day one; the estimates in §5.3 are placeholders.
3. **Retention wording** for OpenAI/Anthropic — link to live policies rather than embedding claims; policies change.
4. **Anthropic `output_config.format` on OpenRouter** — OpenRouter's `/chat/completions` maps `response_format` for Anthropic models, but the direct Anthropic API uses `output_config.format`. If you add a direct Anthropic adapter, implement the native field; don't assume OpenRouter's mapping mirrors it. **[verify before shipping]**
5. **Gemini Interactions API** — a new step-based streaming surface appeared in 2026 docs alongside `generateContent`. If you target it directly, model it as a separate adapter; `generateContent` remains the stable base.
6. **Pricing drift** — all prices are a 2026-10-19 snapshot; refresh the model cache daily and never hardcode prices in the UI.

---

## 9. Sources

**OpenRouter**
- API overview — https://openrouter.ai/docs/api_reference/overview
- Authentication — https://openrouter.ai/docs/api_reference/authentication
- Streaming — https://openrouter.ai/docs/api_reference/streaming
- Errors and debugging — https://openrouter.ai/docs/api_reference/errors-and-debugging
- Limits (credits, budgets, rate limits) — https://openrouter.ai/docs/api_reference/limits
- Provider routing — https://openrouter.ai/docs/guides/routing/provider-selection
- Structured outputs — https://openrouter.ai/docs/guides/features/structured-outputs
- Tool calling — https://openrouter.ai/docs/guides/features/tool-calling
- Reasoning tokens — https://openrouter.ai/docs/guides/best-practices/reasoning-tokens
- Prompt caching — https://openrouter.ai/docs/guides/best-practices/prompt-caching
- ZDR — https://openrouter.ai/docs/guides/features/zdr
- Data collection — https://openrouter.ai/docs/guides/privacy/data-collection
- Provider logging — https://openrouter.ai/docs/guides/privacy/provider-logging
- Models guide / metadata — https://openrouter.ai/docs/guides/overview/models
- Management API keys — https://openrouter.ai/docs/guides/overview/auth/management-api-keys
- Changelog — https://openrouter.ai/docs/changelog
- Live catalog (fetched 2026-10-19) — https://openrouter.ai/api/v1/models

**Anthropic**
- Messages API create — https://platform.claude.com/docs/en/api/messages/create
- Errors — https://platform.claude.com/docs/en/api/errors
- Streaming — https://platform.claude.com/docs/en/build-with-claude/streaming
- Structured outputs — https://platform.claude.com/docs/en/build-with-claude/structured-outputs
- Prompt caching — https://platform.claude.com/docs/en/build-with-claude/prompt-caching
- Tool use — https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview

**Google Gemini**
- generateContent reference — https://ai.google.dev/api/generate-content
- Structured output — https://ai.google.dev/gemini-api/docs/structured-output
- API errors — https://ai.google.dev/gemini-api/docs/api-errors
- Text generation — https://ai.google.dev/gemini-api/docs/text-generation
- Function calling — https://ai.google.dev/gemini-api/docs/function-calling

**OpenAI**
- Structured model outputs — https://developers.openai.com/api/docs/guides/structured-outputs
- Error codes — https://developers.openai.com/api/docs/guides/error-codes
- Prompt caching — https://developers.openai.com/api/docs/guides/prompt-caching
- Models — https://developers.openai.com/api/docs/models

**Local**
- Ollama OpenAI compatibility — https://github.com/ollama/ollama/blob/main/docs/api/openai-compatibility.mdx
- Ollama Anthropic compatibility — https://github.com/ollama/ollama/blob/main/docs/api/anthropic-compatibility.mdx
- LM Studio structured output — https://lmstudio.ai/docs/app/api/structured-output
- llama.cpp GBNF — https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md
- Outlines — https://github.com/dottxt-ai/outlines

**Apple platform**
- TN3137: On Mac keychain APIs and implementations — https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
- kSecUseDataProtectionKeychain — https://developer.apple.com/documentation/security/ksecusedataprotectionkeychain
- kSecAttrAccessibleWhenUnlockedThisDeviceOnly — https://developer.apple.com/documentation/security/ksecattraccessiblewhenunlockedthisdeviceonly

**GEC datasets and evaluation**
- BEA-2019 Shared Task — https://aclanthology.org/W19-4406/
- CoNLL-2014 Shared Task — https://aclanthology.org/W14-1701/
- JFLEG — https://aclanthology.org/E17-2037/
- ERRANT (Bryant, Felice & Briscoe, ACL 2017) — https://aclanthology.org/P17-1074/ ; code: https://github.com/chrisjbryant/errant
