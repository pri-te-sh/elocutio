# Voice Learning — design

**Goal:** Elocutio learns how *you* write — tone, rhythm, diction, punctuation habits, greetings, sign-offs, and the phrases you avoid — and applies that voice to rewrites, drafts, replies, and tone adjustments. It improves from your feedback over time and can learn from writing you upload.

**Status:** design for review
**Owner:** product + analysis engine
**Related:** [PLAN](PLAN.md) · [ARCHITECTURE](ARCHITECTURE.md) · [PROMPTS](PROMPTS.md) · [PRIVACY](PRIVACY.md) · [DECISIONS](DECISIONS.md)

---

## 1. The short version

Elocutio does **not** fine-tune a foundation model on your writing. Instead it builds a **local Voice Profile** from three sources:

1. **Your writing samples** — uploaded files or pasted text.
2. **Your behavior** — which suggestions you accept, dismiss, or edit.
3. **Your explicit feedback** — "sounds like me", trait sliders, preferred/avoided terms.

The profile is a compact, explainable **style card** (structured traits + a short summary + a few representative excerpts). When you ask for a rewrite or draft, Elocutio injects the relevant style card and 2–4 excerpts into the prompt and instructs the model to match the voice without copying content.

This is provider-agnostic, instant, auditable, and private. It improves continuously because the profile is updated from feedback, not retrained.

### Why not fine-tuning?

| Approach | Cost | Latency | Privacy | Provider support | Verdict |
| --- | --- | --- | --- | --- | --- |
| Fine-tune a frontier model | High (training + hosting) | Same | Upload full corpus | Rare; model-specific | No |
| Fine-tune a local model (LoRA) | Medium (GPU/MLX) | Same | Local | Only local models | Later, optional |
| Style card + few-shot exemplars | Near zero | +300–500 prompt tokens | Only card/excerpts leave the Mac | Any model | **v1** |
| Retrieval over samples (RAG) | Low | +1 embedding/search | Excerpts leave the Mac | Any model, if embeddings local | v2 |

Fine-tuning is a poor fit for a BYOK app: the user may switch models or providers at any time, most providers do not expose per-user fine-tuning, and uploading a corpus to train conflicts with the privacy premise. A style card travels with the user across models, is inspectable, and can be deleted instantly. Local LoRA remains a possible power-user feature for Ollama/MLX later.

---

## 2. What "voice" means

A voice is not one thing. Elocutio measures traits in six groups, each with a confidence score and the evidence behind it.

### 2.1 Rhythm

| Trait | How measured | Example |
| --- | --- | --- |
| Average sentence length | Words per sentence, mean and median | 14 words |
| Sentence length variance | Standard deviation / p90 | Medium variation |
| Paragraph length | Sentences per paragraph | 2–3 |
| Fragments | Sentences without a finite verb | Rare |
| Sentence openings | Repeated first words / sentence type | Avoids "I" openings |

### 2.2 Diction

| Trait | How measured | Example |
| --- | --- | --- |
| Contractions | `n't`, `'ll`, `'re`, `'ve`, `'m`, `'d` per 100 words | Frequent |
| Vocabulary richness | Moving-average type-token ratio (MATTR) | Moderate |
| Word length | Mean characters per word | 4.6 |
| Jargon density | Frequency of user glossary terms | High in work profile |
| Preferred terms | Repeated substitutions the user makes | "use" over "utilize" |
| Avoided terms | Repeated deletions/dismissals | "just", "circle back" |

### 2.3 Structure

| Trait | How measured | Example |
| --- | --- | --- |
| Greeting | First non-empty line clustering | "Hi {name}," |
| Sign-off | Last non-empty line clustering | "Thanks,\n{name}" |
| Bullet use | List markers per 100 words | Occasional |
| Headings | Markdown/docx heading density | Rare in email |
| Directness | Imperatives + low hedges | High |
| CTA style | "Can you…" vs "Could you please…" | "Can you…" |

### 2.4 Punctuation and formatting

| Trait | How measured | Example |
| --- | --- | --- |
| Oxford comma | Lists of 3+ with comma before conjunction | Yes |
| Em dash | `—` or spaced hyphen frequency | Rare |
| Semicolon | Frequency per 1,000 words | Rare |
| Exclamation | Frequency per 1,000 words | Low |
| Ellipsis | Frequency | Occasional |
| Parentheses | Frequency | Moderate |
| Emoji | Unicode emoji count | Rare, only in chat |
| Capitalization | All-caps / title-case rate | Sentence case |

### 2.5 Tone

| Trait | How measured | Example |
| --- | --- | --- |
| Formality | Composite of contractions, slang, exclamation, emoji, sentence length | 3.2 / 5 |
| Warmth | Positive lexicon + greeting/sign-off warmth | Medium-high |
| Assertiveness | Hedges, modal verbs, imperative rate | Medium |
| Hedging | "I think", "maybe", "just", "sort of" per 100 words | Low |
| Humor | Lexicon + punctuation (rarely reliable) | Low confidence |
| Urgency | Time words, imperatives, exclamation | Context-dependent |
| Empathy | Second-person + feeling words | Medium |

### 2.6 Person and voice

| Trait | How measured | Example |
| --- | --- | --- |
| First person | "I/we/me/us" rate | Moderate |
| Second person | "you/your" rate | High in email |
| Passive voice | "be" + past participle heuristic | Low |
| Active preference | Imperative/active constructions | High |

Tone labels (formal, friendly, direct, empathetic, etc.) are derived from these traits and, where the user opts in, from a tone classifier. The profile stores both the raw traits and a short natural-language summary.

### 2.7 Rhetorical moves and signature phrases

Statistics capture rhythm; nuances live in *how* the writer handles recurring situations. Elocutio learns a small **phrasebook of moves**:

| Move | What is learned | Example |
| --- | --- | --- |
| Greeting | Preferred opener | "Hi Sam," vs "Hey Sam," |
| Sign-off | Preferred closer | "Thanks!" vs "Best," |
| Apology | How the writer acknowledges a delay/mistake | "Apologies for the late reply" |
| Request | How the writer asks for something | "Could you…" vs "Can you…" |
| Disagreement | How the writer pushes back | "I see it differently…" |
| Gratitude | How the writer thanks | "Thanks for…" vs "Appreciate…" |
| Follow-up | How the writer nudges | "Quick nudge on…" |
| Bad news | How the writer delivers a no/delay | "Unfortunately…" |
| Praise | How the writer recognizes work | "Nice work on…" |
| Scheduling | How the writer proposes times | "Does Thursday work?" |

Detection is local and rule/lexicon-based: identify the speech act by context (subject line, position in message, keywords), then extract the phrase pattern. Store the top 1–3 variants with counts and confidence. Only surface a move after it appears at least 3 times; otherwise it is noise.

**Signature phrases** are 2–4 word sequences that appear more often than expected in the user's samples (log-lift against a baseline corpus or against the user's other samples). Keep the top 5–10, exclude generic filler, and filter out proper nouns, names, email addresses, phone numbers, URLs, and product-specific secrets so the phrasebook does not become a content leak. Use them sparingly:

- In prompts, list at most 3–5 moves and 3 signature phrases.
- Never force a move if the situation does not call for it.
- Do not insert signature phrases verbatim; they are style guidance, not templates.
- Let the user edit, disable, or delete any move or phrase.

Examples in a style card:

```text
Preferred moves:
- apology: "Apologies for the late reply"
- request: "Could you take a look at…"
- sign-off: "Thanks!"
Signature phrases: "quick update", "on track for", "happy to help"
```

This is what makes the profile feel like a person rather than a formality score: it learns the small, repeated choices that carry tone.

---

## 3. Where the data comes from

| Source | What it teaches | Consent |
| --- | --- | --- |
| **Uploaded samples** — `.txt`, `.md`, `.docx` (PDF/RTF later), or pasted text | Rhythm, diction, structure, punctuation, greetings, sign-offs | Explicit; user selects files and reviews extracted text |
| **Accepted suggestions** | Which edits the user agrees with | Implicit; only when voice learning is enabled |
| **Dismissed suggestions** | Which edits the user rejects; categories to suppress | Implicit; only when voice learning is enabled |
| **Edits after accepting** | The strongest signal: the user changed the model's wording | Implicit; only when voice learning is enabled; weighted higher |
| **Tone selections** | Preferred formality/warmth for a context | Explicit |
| **"Sounds like me" ratings** | Direct feedback on generated text | Explicit |
| **Trait sliders / overrides** | User corrections to the profile | Explicit; always wins |
| **Preferred/avoided terms** | Diction rules | Explicit or learned from repeated edits |

### 3.1 Two independent switches

Keep consent granular:

| Switch | Default | Controls |
| --- | --- | --- |
| **Learn from my edits** | Off | Accepted/dismissed/edited suggestions update the profile over time |
| **Use my voice** | Off | The profile is injected into rewrites, compose, and replies |

A user can upload samples once, use the voice, and never allow continuous learning. Or allow learning from edits without uploading anything. The two are independent.

**Consent moments:**

1. Onboarding: an optional "teach your voice" step with a clear explanation.
2. After 5 accepted edits: a one-time, dismissible card — "Want Elocutio to learn from edits like these?" with "Learn from edits" / "Not now."
3. Before the first voice-aware request: a preview of the exact style card and exemplars that will be sent.

No silent learning. No default-on collection.

**What Elocutio does not learn from:**

- AI-generated text the user accepted **without editing** (weight 0.1 or exclude). Otherwise the model's voice would contaminate the profile.
- Other people's writing, unless the user confirms they have permission and it is a brand/team voice.
- Text from apps/sites the user marked "don't learn from".
- Secure fields, password managers, terminals.
- Deleted samples.

---

## 4. Ingestion pipeline

```text
Upload / paste
      │
      ▼
Parse locally (txt/md/docx; pdf/rtf later)
      │
      ▼
Normalize: encoding, line endings, whitespace
      │
      ▼
Detect language; reject mismatches
      │
      ▼
Strip boilerplate: email headers, quoted replies, signatures (optional)
      │
      ▼
Quality gate: ≥100 words, not code/lists, not gibberish
      │
      ▼
Deduplicate by SHA-256
      │
      ▼
PII redaction preview (optional)
      │
      ▼
Extract features + excerpts
      │
      ▼
Update sufficient statistics → profile → style card
```

### 4.1 File support

| Format | v1 | Parser | Notes |
| --- | --- | --- | --- |
| `.txt` | ✅ | UTF-8 / UTF-16 detection | Most reliable |
| `.md` / `.markdown` | ✅ | Raw text | Preserve headings for structure traits |
| `.docx` | ✅ | `zip` + `quick-xml` (`word/document.xml`) | Extract `<w:t>`, paragraph breaks |
| Paste | ✅ | — | Always available |
| `.pdf` | Later | `pdf-extract` or PDFKit helper | Quality varies; warn |
| `.rtf` | Later | RTF parser | Rare |
| `.html` | Later | `scraper` / `ammonia` | Strip boilerplate |
| Email export (`.eml`/`.mbox`) | Later | MIME parser | Useful but noisy |
| Google Docs | Never directly | — | Export to `.docx` and upload |

Limits: 10 MB per file, 100 files per profile, 200,000 words per profile, 20 MB stored per profile. Warn before exceeding.

### 4.2 Quality and selection

- Minimum 100 words per sample.
- Exclude code blocks, tables, link lists, and boilerplate.
- Prefer samples the user wrote, not forwarded content.
- Detect quoted replies (`>` lines, "On … wrote:") and offer to strip.
- Detect signatures and offer to keep or strip (sign-offs are useful voice signals; contact details are not).
- Language detection (`whatlang`); a profile is language-specific. A bilingual user can have two profiles.
- Deduplicate identical or near-identical samples (SimHash/MinHash).

### 4.3 Excerpt selection (few-shot exemplars)

For prompt use, pick 2–4 excerpts per profile:

- 40–120 words each.
- From user-authored samples, not AI-generated text.
- Diverse in length and tone (cluster by sentence length/formality; pick medoids).
- Context-matched (email excerpts for email rewrites).
- No sensitive content; user can flag or delete.
- Store with the sample ID so deletion cascades.
- Total prompt budget ≤500 tokens.

The full sample is used for feature extraction, not dumped into the prompt.

---

## 5. Feature extraction and profile building

Implementation: Rust crate `elocutio-voice`, using `unicode-segmentation`, `whatlang`, `regex`, and a small lexicon. No network calls.

### 5.1 Sufficient statistics

For each trait, store counts/sums rather than recomputing from raw text:

```rust
pub struct VoiceStats {
    pub words: u64,
    pub sentences: u64,
    pub sentence_words_sum: u64,
    pub sentence_words_sq_sum: u64,
    pub paragraphs: u64,
    pub contraction_count: u64,
    pub first_person_count: u64,
    pub second_person_count: u64,
    pub passive_count: u64,
    pub hedge_count: u64,
    pub emoji_count: u64,
    pub exclamation_count: u64,
    pub semicolon_count: u64,
    pub em_dash_count: u64,
    pub oxford_comma_hits: u64,
    pub oxford_comma_opportunities: u64,
    pub greeting_counts: HashMap<String, u64>,
    pub signoff_counts: HashMap<String, u64>,
    pub preferred_terms: HashMap<String, u64>,
    pub avoided_terms: HashMap<String, u64>,
    pub last_updated: i64,
}
```

### 5.2 Confidence

Per-trait confidence reflects sample size and variance:

- Binary traits (Oxford comma, contractions): Beta posterior; confidence = 1 − posterior variance scaled.
- Rates (hedges, passive): Wilson interval half-width; confidence = 1 − normalized half-width.
- Continuous (sentence length): standard error; confidence = clamp(1 − SE/mean, 0, 1).

Display bands:

| Sample words | Confidence | UI |
| --- | --- | --- |
| <300 | Low | "Add more writing to improve accuracy" |
| 300–1,500 | Medium | "Getting there" |
| 1,500–5,000 | High | "Good" |
| >5,000 | Very high | "Strong" |

Per-trait confidence is shown, not just a global number.

### 5.3 Updating over time

- Each new sample or weighted feedback event updates sufficient statistics.
- Exponential decay with a **180-day half-life** so the profile follows the user's evolution without abrupt swings:

```text
weight = 0.5 ^ (days_since_event / 180)
stats = stats * weight + new_event
```

- Explicit overrides always win over learned values.
- Drift detection: compare the last 30 days against the historical profile. If the mean formality or sentence length shifts beyond a threshold, show "Your recent writing looks different — update your voice?" and let the user accept or keep the old profile.
- Version the profile (`voice_profile_version`) so prompt changes and migrations are traceable.
- Cap the influence of any single sample (e.g., max 20% of total weight) to avoid one unusual document dominating.

### 5.4 Optional model-assisted analysis

Local heuristics are good at rhythm, punctuation, and diction, but weak at humor, sarcasm, empathy, and rhetorical nuance. Offer an **optional** enrichment pass:

| Mode | What happens | Privacy | Default |
| --- | --- | --- | --- |
| Heuristics only | Local feature extraction | No sample leaves the Mac | ✅ |
| On-device model | Apple Foundation Model (macOS 26) or Ollama labels short samples | No network | Off |
| Cloud model | Cheap model labels tone/humor/empathy on selected samples | Sample text goes to the provider | Off |

Cloud enrichment requires per-sample consent, a preview of the exact payload, and a cost estimate. The model returns labels and short rationales, not rewritten text; the local profile remains the source of truth. Never send a whole corpus in one request; analyze a few representative samples.

If the user declines, the heuristic profile still works. The UI marks traits that would benefit from enrichment ("humor: low confidence — analyze a sample to improve").

### 5.5 What the profile stores

```json
{
  "version": 2,
  "profile_id": "vp_work_email",
  "context": "email",
  "language": "en",
  "sample_words": 12400,
  "confidence": 0.86,
  "traits": {
    "avg_sentence_words": { "value": 14.2, "confidence": 0.92 },
    "sentence_length_variance": { "value": "medium", "confidence": 0.81 },
    "contractions": { "value": "frequent", "confidence": 0.88 },
    "formality": { "value": 3.2, "scale": "1-5", "confidence": 0.74 },
    "warmth": { "value": "medium-high", "confidence": 0.69 },
    "hedging": { "value": "low", "confidence": 0.83 },
    "first_person": { "value": "moderate", "confidence": 0.77 },
    "emoji": { "value": "rare", "confidence": 0.94 },
    "oxford_comma": { "value": true, "confidence": 0.91 },
    "em_dash": { "value": "rare", "confidence": 0.72 },
    "greeting": { "value": "Hi {name},", "confidence": 0.85 },
    "signoff": { "value": "Thanks,\n{name}", "confidence": 0.8 }
  },
  "moves": {
    "apology": [{ "phrase": "Apologies for the late reply", "count": 4, "confidence": 0.78 }],
    "request": [{ "phrase": "Could you take a look at…", "count": 9, "confidence": 0.88 }],
    "signoff": [{ "phrase": "Thanks!", "count": 14, "confidence": 0.93 }]
  },
  "signature_phrases": ["quick update", "on track for", "happy to help"],
  "prefer": [{ "from": "utilize", "to": "use" }],
  "avoid": ["circle back", "just wanted to"],
  "notes": "Direct and warm; short paragraphs; low hedging; avoids exclamation marks; uses contractions.",
  "exemplars": [
    { "id": "ex_1", "text": "Hi Sam — quick update on the launch...", "words": 64 },
    { "id": "ex_2", "text": "Thanks for the detailed notes. I agree with...", "words": 58 }
  ]
}
```

The `notes` field is generated from a template, not an LLM, so no sample text leaves the Mac during profile building.

---

## 6. How the profile is used

### 6.1 Task-specific voice strength

| Task | Voice use | Rationale |
| --- | --- | --- |
| Proofread / grammar | **Off** | Grammar is voice-neutral; the profile would add noise |
| Clarity / conciseness | **Light** | Preserve voice while improving clarity |
| Rewrite / tone adjustment | **Balanced** | Match rhythm, diction, punctuation; target tone wins on tone |
| Compose / reply | **Strong** | The draft should sound like the user from the first sentence |
| Summarize | **Light** | Preserve voice in summaries where appropriate |
| Translate | **Light** | Approximate voice in the target language; naturalness first |
| Chat | **Off by default** | Chat is a conversation with the assistant, not a writing surface |

### 6.2 Prompt injection

The style card is injected as a compact XML block:

```text
<voice_profile context="email" confidence="0.86" strength="balanced">
Style: direct and warm; short paragraphs; low hedging; uses contractions; avoids exclamation marks.
Sentences: average 14 words, medium variation.
Punctuation: Oxford comma yes; em dash rare; semicolon rare.
Greeting: "Hi {name},". Sign-off: "Thanks,\n{name}".
Prefer: "use" over "utilize".
Avoid: "circle back", "just wanted to".
Examples of the writer's voice (match the style; do not copy content, names, or facts):
1. "Hi Sam — quick update on the launch. We're on track for Thursday..."
2. "Thanks for the detailed notes. I agree with the first two points..."
</voice_profile>
```

The task prompt adds:

```text
Match the voice in <voice_profile> for rhythm, diction, punctuation, and structure.
Do not copy content, names, numbers, or facts from the examples.
Do not invent details to sound like the examples.
If the requested tone conflicts with the voice, the requested tone wins for tone;
the voice still guides rhythm and diction.
```

### 6.3 Prompt budget

| Strength | Card | Exemplars | Total |
| --- | --- | --- | --- |
| Off | 0 | 0 | 0 |
| Light | ~120 tokens | 0 | ~120 |
| Balanced | ~150 tokens | 2 × 80 | ~310 |
| Strong | ~180 tokens | 4 × 80 | ~500 |

The profile is omitted for short texts where it would dominate the prompt (e.g., <40 words) unless the user explicitly selects "use my voice."

### 6.4 Conflict resolution

- **Target tone vs voice:** target tone wins for formality/warmth; voice wins for rhythm, diction, punctuation.
- **Brand voice vs personal voice:** brand voice wins in brand contexts; show which profile is active.
- **Style guide vs voice:** style guide rules are hard constraints; voice is a soft guide.
- **Voice vs clarity:** clarity wins; do not preserve a confusing construction just because it is characteristic.

---

## 7. Learning from behavior

### 7.1 Signals and weights

| Event | Weight | What it updates |
| --- | --- | --- |
| User types text (captured on analyze) | 1.0 | All traits; only when the user opts in to learning from typing |
| User edits an accepted suggestion | 0.7 | Diction, punctuation, preferred terms |
| User accepts a suggestion unchanged | 0.1 | Weak positive; do not let AI phrasing dominate |
| User dismisses a suggestion | 0.5 (negative) | Category suppression, avoided terms |
| User selects a tone | 0.6 | Tone traits for the context |
| "Sounds like me" 1–5 | (rating−3)/2 | Global alignment; low ratings trigger review |
| Trait slider override | ∞ | Explicit value; wins until reset |
| Preferred/avoided term added | ∞ | Diction rules |

### 7.2 Category weighting

Not every accepted edit is a voice signal:

| Category | Weight multiplier | Why |
| --- | ---: | --- |
| Spelling, punctuation, capitalization | 0.3 | Correctness, not style; only teaches preferred terms/spelling dialect |
| Grammar | 0.4 | Mostly correctness; repeated changes may indicate dialect preferences |
| Clarity, conciseness | 0.7 | Mild style signal |
| Word choice, style | 1.0 | Strong voice signal |
| Tone | 1.0 | Strong voice signal |

Dismissals of grammar suggestions should be treated as **rule preferences** (e.g., the user prefers a dialect or a construction), not as general voice evidence. Store them separately so they do not distort the tone profile.

### 7.3 Preference inference

- If the user repeatedly removes "just" or "I think", add to `avoid` after 3 consistent events.
- If the user repeatedly changes "very" to "really", add a preferred-term pair after 3 events.
- If the user dismisses a category (e.g., word-choice) in a context, reduce its weight or disable it for that context after 5 dismissals.
- Never infer sensitive attributes (gender, age, ethnicity, personality) from writing. The profile is about style, not identity.
- Show every inferred preference in the UI with an undo.

### 7.4 Feedback-loop protection

- AI-generated text accepted without edits is down-weighted, so the profile does not converge on the model's voice.
- The profile is updated from **user-authored** text and **user edits**, not from model output.
- Cap the rate of change: a single session cannot move a trait by more than a small delta; require repeated evidence.
- Provide a "Freeze profile" option for users who are happy with it.

---

## 8. Contexts and multiple voices

### 8.1 Contexts

| Context | Detected from | Default voice behavior |
| --- | --- | --- |
| Email | Mail, Outlook, Gmail, Superhuman | Balanced/Strong |
| Chat | Slack, Teams, Discord, Messages | Light |
| Document | Word, Pages, Obsidian, Docs editor | Balanced |
| Social | LinkedIn, X, Facebook | Balanced |
| Support | Zendesk, Intercom, Help Scout | Balanced |
| General | Everything else | Light |

Detection uses bundle ID/site mapping; the user can override per app/site.

### 8.2 Profiles

- Start with one **Personal** profile.
- Add **Work**, **Brand**, or client profiles.
- Each profile has its own samples, traits, exemplars, and confidence.
- A profile can be scoped to a context (e.g., "Work email") or shared across contexts.
- Per-app/site mapping selects the active profile.
- Export/import as JSON for backup or team sharing (no cloud sync in v1).

### 8.3 Brand voice

- A brand profile can be created from approved brand copy.
- The user must confirm they have permission to use the material.
- Brand profiles are marked as such and are not used for personal contexts unless selected.
- Team sharing is export/import in v1; a shared library is post-1.0.

---

## 9. UI

### 9.1 Settings → Voice

```text
┌─────────────────────────────────────────────────────────────┐
│  My Voice                                      [Preview]    │
│  Work email · 12,400 words · Strong confidence              │
│  Direct and warm; short paragraphs; low hedging.            │
│                                                             │
│  Voice strength:  Off   Light   [Balanced]   Strong         │
│                                                             │
│  [ Add writing samples ]   [ Review what Elocutio learned ]     │
│                                                             │
│  Contexts:  Email  Chat  Documents  Social  Support         │
│  Profiles:  Personal · Work · Brand                         │
└─────────────────────────────────────────────────────────────┘
```

### 9.2 Teach Elocutio your voice (wizard)

1. **Choose a context** (email, chat, documents, social, support).
2. **Add samples** — drag files, choose files, or paste text.
3. **Review extracted text** — trim signatures/quoted replies; see redactions.
4. **See what Elocutio learned** — traits with confidence and evidence; edit or override.
5. **Preview** — generate a short rewrite in your voice; rate "sounds like me"; adjust strength.

### 9.3 Review what Elocutio learned

- Trait list grouped by rhythm/diction/structure/punctuation/tone.
- Each trait: value, confidence, examples/evidence, override control.
- Phrases: learned moves (greeting, apology, request, sign-off…) and signature phrases, each with count and an enable/delete control.
- "Learned from your edits" section: counts and top preferences.
- "What will be sent" preview: the exact style card and exemplars.
- Buttons: Freeze profile, Reset learned values, Delete samples, Export JSON.

### 9.4 In-flow controls

- Rewrite menu: "Use my voice" toggle (defaults per task).
- Assistant panel: "Match my voice" chip.
- Suggestion card: small "Voice: balanced" indicator when a rewrite uses the profile.
- "This doesn't sound like me" action on any generated text → records a low rating and opens the trait editor.

### 9.5 Empty and low-confidence states

- No samples: "Elocutio can learn your voice from 3–5 emails or documents. Add samples, or let it learn from your edits over time."
- Low confidence: "Add about 500 more words for a reliable profile."
- No permission to learn from typing: explain the trade-off; uploads still work.
- Deleted profile: confirm; derived data removed.

---

## 10. Data model

See [`ARCHITECTURE.md`](ARCHITECTURE.md) §12 for the full schema. Voice-specific tables:

```sql
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
  text_encrypted BLOB,              -- AES-256-GCM; optional if "features only"
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

Indexes: `voice_samples(profile_id)`, `voice_events(created_at)`, `voice_exemplars(profile_id)`.

---

## 11. Privacy and security

| Data | Storage | Sent to provider? |
| --- | --- | --- |
| Raw samples | Local, encrypted at rest (AES-256-GCM, key in Keychain) | **No** by default |
| Derived traits/style card | Local SQLite | **Yes**, when a voice-aware task runs |
| Exemplars | Local; user-reviewed excerpts | **Yes**, 2–4 excerpts at Balanced/Strong |
| Feedback events | Local SQLite | No |
| Deleted samples | Hard-deleted; derived stats recomputed or marked stale | No |

Controls:

- Voice learning is **off by default**; the user opts in.
- "Use my voice" can be disabled per task.
- "Style card only" mode never sends excerpts.
- "Show what will be sent" preview before the first voice-aware request.
- Per-app "don't learn from this app."
- Delete a sample → its exemplars and stats contributions removed.
- Delete profile → all samples, exemplars, events, and derived data removed.
- Export/import JSON; no cloud sync.
- No cross-user learning, no telemetry.
- Encrypt samples at rest; file permissions 0600.
- Redaction option for emails, phone numbers, URLs, and names before storing/analyzing.
- Impersonation guard: the user must attest they wrote the samples or have permission; brand profiles require a confirmation.

**Threat model additions**

| Threat | Mitigation |
| --- | --- |
| Sensitive samples leak via prompt exemplars | Excerpt-only, preview, style-card-only mode, redaction |
| Stolen laptop exposes samples | AES-GCM encryption, Keychain-wrapped key, FileVault |
| Model copies sample content | 8-word n-gram overlap guard; reject/regenerate |
| Profile used to impersonate someone | Consent attestation; brand profile labeling; no "clone a person" feature |
| Feedback loop converges on AI voice | Down-weight unedited AI text; learn from user edits |
| Deleted data persists in backups | Document that backups are outside app control; provide "delete all voice data" |

---

## 12. Quality evaluation

### 12.1 Offline metrics

Split samples 80/20. Generate rewrites of neutral prompts with and without the profile. Compare:

| Metric | Target |
| --- | --- |
| Sentence-length distribution similarity | Within 15% of held-out mean |
| Contraction rate similarity | Within 0.1 per 100 words |
| Formality score similarity | Within 0.4 / 5 |
| Punctuation rate similarity | Within 20% |
| Greeting/sign-off match | ≥80% for email |
| Preferred-move match (apology/request/sign-off) | ≥70% of held-out cases |
| 8-gram overlap with samples | 0 |
| Meaning preservation | ≥98% |
| New facts/entities | 0 |

### 12.2 Human metrics

- "Sounds like me" 1–5; target ≥4.0 after 3 samples.
- Blind A/B: voice vs no voice; target ≥65% preference.
- Edit distance after generation; target ≤20% of words changed.
- Diversity: different prompts should not produce identical phrasing.

### 12.3 Regression suite

- 20 neutral prompts × 5 contexts × 3 profiles (terse, warm, formal).
- Copy-detection tests with deliberately distinctive sample phrases.
- Feedback-loop test: accept 50 AI rewrites unchanged; assert the profile does not drift toward the model.
- Deletion test: delete a sample; assert its exemplars and stats are gone.
- Encryption test: raw DB without the Keychain key does not reveal sample text.

---

## 13. Failure modes and guardrails

| Failure | Symptom | Guardrail |
| --- | --- | --- |
| Caricature | Output exaggerates one trait ("synergy" everywhere) | Cap trait influence; blend with neutral; strength slider |
| Overfitting to one document | Profile swings after one upload | Cap single-sample weight; confidence thresholds |
| Copying | Output repeats sample sentences | 8-gram overlap guard; reject/regenerate |
| Meaning drift | Facts/numbers change | Entity/number invariant check |
| Feedback loop | Output sounds like the model, not the user | Learn from user-authored text/edits; down-weight accepted AI text |
| Cross-context leakage | Work jargon in personal email | Per-context profiles; context detection; override |
| Privacy surprise | User didn't realize excerpts are sent | Preview + style-card-only mode + clear UI |
| Stale profile | Voice changed after a job change | Drift detection; "update voice?" prompt |
| Bad parsing | Garbage traits from a PDF/table | Quality gate; preview extracted text; reject |
| Impersonation | User uploads someone else's writing | Attestation; brand profile labeling; acceptable-use policy |

---

## 14. Phasing and effort

| Phase | Scope | Effort |
| --- | --- | --- |
| **V0 — M2** | Profile from accepted/dismissed/edited suggestions; manual trait overrides; style card; apply to rewrites; "sounds like me" rating; UI | 3–4 days |
| **V1 — M3** | Upload `.txt`/`.md`/`.docx` + paste; feature extraction; rhetorical moves + signature phrases; exemplars; preview; delete; encrypted storage | 4–5 days |
| **V2 — M4** | Per-context and multiple profiles; retrieval; drift detection; export/import; PDF; brand voice; freeze | 4–5 days |
| **V3 — later** | Local LoRA fine-tuning for Ollama/MLX; voice blending; team library | post-1.0 |

Total: **11–14 engineering days**, folded into M2–M4. This moves v1.0 from ~12–16 weeks to **~14–19 weeks**.

---

## 15. Open questions

1. **Raw sample storage:** encrypted raw text (reprocessable) vs features-only (more private). Recommend encrypted raw with a "features only" option.
2. **PDF priority:** is `.docx` + paste enough for v1? Recommend yes; PDF in M4.
3. **Local embeddings:** use Apple `NLEmbedding` or a small local model for exemplar retrieval, or feature-based selection? Start feature-based; embeddings in V2.
4. **Voice blending:** 70% work + 30% personal? Defer; allow multiple profiles instead.
5. **Brand voice sharing:** export/import JSON in v1; hosted sync later.
6. **Local LoRA:** worth it for Ollama users? Evaluate after v1; the style card may be sufficient.
7. **Typing capture:** learn from everything typed (privacy-sensitive) vs only analyzed text and uploads. Recommend only analyzed text + explicit samples by default.
8. **Language coverage:** voice traits are English-first; LLM voice matching works for other languages, but local metrics need per-language lexicons. Start English; add Spanish/French/German later.

---

## 16. Definition of done

- A user can add 3 email samples, see a clear style card, preview a rewrite, and rate it "sounds like me."
- The profile updates from accepted/dismissed/edited suggestions and explains what changed.
- Voice-aware rewrites match held-out style metrics within the targets above.
- No sample content is copied; the 8-gram guard passes.
- Raw samples are encrypted at rest; "style card only" sends no excerpts.
- Deleting a sample/profile removes its derived data.
- The UI shows exactly what will be sent before the first voice-aware request.
- The feature is off by default and can be disabled per task/app.
