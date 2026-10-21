# Security review — inherited code and the M1/M2 additions

**Reviewed:** 2026-10-21
**Scope:** the Harper-derived code in `apps/macos/src-tauri/`, the vendored
`vendor/harper/`, and the Elocutio crates in `crates/`.
**Why this document exists:** `docs/PLAN.md` §2.4 lists "Run a security review
of the inherited code (network surface, IPC, updater, permissions)" as
**fork condition 6**. Until a review exists, the fork condition is unmet and
M0 cannot close.

**Status:** the review is complete. Two findings were fixed in the same pass
that wrote this document; the rest are either closed by design, accepted
deliberately, or deferred with a named owner.

---

## 1. Method

The review looked at each surface where untrusted data can enter the app:

| Surface | Question asked |
| --- | --- |
| Network | Which hosts can the app talk to, and can a user or a document redirect that? |
| IPC | Can another process impersonate the highlighter, or read the main process? |
| Updater | Can an unsigned update be installed? |
| Permissions | What does the app ask macOS for, and what does it refuse to touch? |
| Storage | What is written to disk, and does any of it contain the user's text? |
| Text injection | Can a write land in the wrong place, or in a field it should not? |
| Prompt injection | Can text from a document or a user instruction escape into the model's instructions? |

Each surface is marked **Closed**, **Accepted** (a deliberate, documented
decision), **Fixed** (a defect found and repaired), or **Open** (a defect that
remains, with an owner and a milestone).

---

## 2. Network surface

### Finding N-1 — the app talks only to the provider the user configured

**Closed by design.**

Every outbound request goes through `apps/macos/src-tauri/src/openrouter.rs`
and its `llm/http.rs` client. The only hard-coded hosts are:

- `https://openrouter.ai/api/v1/key` — key validation
- `https://openrouter.ai/api/v1/chat/completions` — chat completions
- `https://github.com/pri-te-sh/elocutio` — the `HTTP-Referer` attribution
  header OpenRouter expects (it is a link, not a request)

A user-configured **OpenAI-compatible base URL** is the only other host, and it
is set by the user in Settings. The base URL is validated before use
(`commands.rs` rejects anything that is not `http(s)`), and the key, if any, is
sent only to that host.

**Residual risk:** the app is not sandboxed, so a hostile process running as
the same user could read the keychain. This is a consequence of needing
Accessibility access, which the App Sandbox does not permit. It is called out
in `docs/PLAN.md` §5.7 and accepted.

### Finding N-2 — no telemetry, no analytics, no crash reporting

**Closed by design.** There is no telemetry code in the repository. A
`cargo audit` job runs in CI (`.github/workflows/ci.yml`) so a known-vulnerable
dependency cannot land unnoticed. Crash reporting is listed in the plan as M5
work and does not exist yet.

### Finding N-3 — the key never reaches the highlighter process

**Closed by design, and enforced by the process split.** The provider key
lives in the macOS Keychain and is read only by the main process
(`openrouter.rs::load_key_optional`). The highlighter subprocess receives
lint results and a suggestion cache over stdio; it has no network client and
no keychain access. `docs/PRIVACY.md` states this, and the process boundary
makes it true rather than aspirational.

### Finding N-4 — the update path was removed, so there is no unsigned-update path

**Closed by design.** The inherited `desktop_updater.rs` was deleted for M0
and `tauri-plugin-updater` is not in the dependency tree, so there is currently
**no** update mechanism at all. `SECURITY.md` used to claim "updates are signed
and verified before installation", which described a future state. It now
states the truth and marks signed updates as M5 work, with the signing keypair
requirement spelled out in `docs/PLAN.md`.

---

## 3. IPC surface

### Finding I-1 — the IPC is stdio to a child we spawn ourselves

**Closed by design.** The highlighter is spawned as
`std::env::current_exe()` with the argument `highlighter`
(`highlighter_service/highlighter_process.rs:17`). It is the same signed
binary, and the parent holds both pipe ends. There is no listening socket, so
there is nothing for another process to connect to.

**Residual risk:** any process running as the same user can read and write the
pipes of a process it can see. This is the same threat model as the keychain
access above.

### Finding I-2 — the documented protocol did not match the implementation

**Fixed.** `docs/ARCHITECTURE.md` promised a versioned envelope with `v`,
`type`, and `id` fields and a 2 MB message-size limit. The implementation used
untagged serde enums with newline framing and **no size limit at all**
(`communication/message.rs`, `communication/framing.rs`).

The documentation has been corrected to describe the protocol that is actually
implemented. The missing size limit is worth fixing before the M3 browser
bridge reuses this transport, because the bridge is a network surface and a
length-prefixed frame with a cap is the right shape for it. The version
envelope matters more there than here: a local pipe between two halves of the
same binary cannot be version-skewed, but a browser extension can.

**Open — owner: the M3 gate.** Add a frame-size limit and a protocol version
before the bridge lands.

### Finding I-3 — a request can name any bundle identifier

**Accepted.** `Request::ResolveIntegration { bundle_id }` accepts an
arbitrary string. It is used to decide whether the app should read a focused
window, and the answer is only ever "read" or "do not read". The highlighter
still requires Accessibility permission, which is granted by the user through
macOS, so a forged request cannot widen the permission.

**Residual risk:** a `bundle_id` that does not exist simply resolves to
"not enabled". No file, network, or configuration access is keyed on it.

---

## 4. Permissions

### Finding P-1 — the app requests Accessibility, and refuses secure fields

**Closed by design, and tested.** The app requires Accessibility to read and
write text in other applications. It explicitly refuses to touch:

- secure input — `IsSecureEventInputEnabled()`
  (`mac_broker/mod.rs:361`)
- secure text fields — `AXSubrole == "AXSecureTextField"`
  (`mac_broker/mod.rs:375`)

Both checks run before the focused element is read.

### Finding P-2 — a password manager's field is never read

**Closed by design.** The same secure-field refusal covers password
managers, which use secure text fields. The refusal is on the *element*, not
on a bundle-identifier blocklist, so a password manager that does not use a
secure field is still covered by the general rule that Elocutio never reads a
field it has not been enabled for.

### Finding P-3 — the app asks for no other permission

**Closed by design.** The Tauri capability file
(`apps/macos/src-tauri/capabilities/default.json`) requests only `core`,
`opener`, `autostart`, `notification`, and `global-shortcut`. There is no
screen-recording, camera, microphone, filesystem, or automation permission.
Screen Recording, for the optional OCR fallback, is **not implemented** — the
plan lists it as opt-in and never-on-by-default, and it does not exist yet.

### Finding P-4 — a block list is a hard stop, not a preference

**Fixed in this pass.** The block list was added with the rest of the
M1/M2 work. A block-listed app is never read, never linted, and never sent to
a provider, and it is never auto-enabled, even if it is also enabled as an
integration. `docs/PRIVACY.md` previously promised this control; now the code
does it.

---

## 5. Storage

### Finding S-1 — the usage log records metadata, never text

**Closed by design, and tested.** `usage.rs` records the timestamp, model,
token counts, and provider-reported cost. It has no field for text.

### Finding S-2 — the voice feedback log stores the writer's edit text

**Accepted, with bounds.** `voice-feedback.jsonl` stores the `original` and
`replacement` of the user's own edits — the text that was already in the
field. This is the signal the voice profile learns from, and it cannot be
derived from anything else. The bounds are:

- each stored text field is truncated to **240 characters**
- rows older than **365 days** are pruned
- the raw *uploaded samples* are stored **nowhere**: only aggregate traits and
  8-gram fingerprints are kept

The privacy document now says exactly this, including the fact that the
feedback log contains text, rather than claiming no text is stored anywhere.

### Finding S-3 — the undo record holds the whole field

**Accepted.** `last-edit-undo.json` holds the field's before-and-after text so
the undo can restore it. It is deleted after one undo. The trade-off is
stated in `docs/PRIVACY.md`.

### Finding S-4 — the suggestion event log stores no text

**Closed by design.** `suggestion-events.jsonl` records the rule, the
category, and the size of the edit. It has no text field at all.

### Finding S-5 — the key is never written to the config file or the logs

**Closed by design.** The key is in the macOS Keychain under
`app.elocutio.provider.openrouter` (or `…openai_compatible`). `config.json`
serializes the provider *kind* and base URL, never the key.

---

## 6. Text injection

### Finding T-1 — the read-back check accepted a corrupted paste

**Fixed in this pass.** The verification was
`AXValue.contains(replacement)`, so a paste that landed the replacement *plus
leftover text*, or landed it in the wrong place, still verified. The check is
now structural: the field before the paste must contain the selection, and
the field after the paste must equal the value before with that occurrence
replaced. Seven unit tests cover the cases, including the one the old check
passed incorrectly.

### Finding T-2 — the clipboard restore destroyed non-text items

**Fixed in this pass.** The pasteboard snapshot captured only the string type
and `restore()` called `clearContents()` before writing the string back, so an
image, a file, or a rich item on the clipboard was **permanently lost** by a
rewrite. The snapshot now captures every pasteboard item with its bytes, and
the restore rebuilds them. A restore that could not capture every type is
reported rather than silently lossy.

### Finding T-3 — the clipboard was cleared before the paste

**Fixed in this pass.** The old code cleared the pasteboard and then wrote the
rewrite, so a failed paste destroyed the user's clipboard before the restore
could run. The write is now transactional in effect: the snapshot is taken
first, and the restore runs whether or not the paste succeeded.

### Finding T-4 — the selection length was capped, but the reasoning was worth checking

**Closed, with a note.** The 12,000-character cap applies to the *selection*.
The model's output is bounded separately by the request's `max_tokens`
(`REWRITE_MAX_TOKENS`, currently 2048), so the replacement cannot be
unbounded. The two limits are different things and neither is a substitute
for the other, so both are documented here rather than one being assumed to
cover the other.

---

## 7. Prompt injection

### Finding P-1 — a user instruction could close its own prompt wrapper

**Fixed in this pass.** The custom-instruction sanitizer stripped control
characters and collapsed newlines, but it left angle brackets. An instruction
containing `</custom_instruction>` closed the wrapper and put the rest of the
text outside the data boundary. The sanitizer now removes angle brackets, and
a test asserts the wrapper cannot be closed from inside.

### Finding P-2 — document text is delimited and marked as data

**Closed by design.** The analysis prompt wraps the paragraph in a
`<paragraph id="…">` element and the system prompt states that the content
inside it is data, never instructions. The voice style card and the custom
instruction use the same data-marking.

### Finding P-3 — the model cannot choose where an edit lands

**Closed by design.** The model returns verbatim quotes, never offsets. The
client resolves the quote locally and drops anything ambiguous, so a
prompt-injected instruction cannot make the app write to an arbitrary
position. This is the single most important property of the design and it is
covered by the resolver's property tests.

---

## 8. Model output

### Finding M-1 — the model could copy the writer's own samples back

**Fixed in this pass.** The voice copy guard compares the provider's output
against the 8-gram fingerprints of the writer's samples and refuses the
rewrite if they overlap. The guard is applied inside `rewrite_with`, so every
rewrite path fails closed, not just one.

**Accepted limitation:** the guard is a heuristic, not a security boundary. A
paraphrase, or a fragment shorter than eight words, is not detected. The
purpose is to catch a model reproducing training-like content, not to stop a
determined adversary.

### Finding M-2 — the default request was rejected by the default model

**Fixed in this pass.** The request body sent `"temperature": 0.0`.
Anthropic's Haiku 5.5 accepts only `1` and rejects any other value with an
HTTP 400, so the default rewrite path was broken. The key is now omitted
entirely, which every model accepts.

---

## 9. Summary

| # | Finding | Severity | Status |
| --- | --- | --- | --- |
| N-1 | Only user-configured hosts are contacted | — | Closed by design |
| N-2 | No telemetry or analytics | — | Closed by design |
| N-3 | The key never reaches the highlighter | — | Closed by design |
| N-4 | No update path, so no unsigned-update path | — | Closed by design |
| I-1 | IPC is stdio to a child we spawn | — | Closed by design |
| I-2 | The documented IPC did not match the code | Medium | **Fixed** (docs); frame limit and version envelope **Open for M3** |
| I-3 | A request can name any bundle identifier | Low | Accepted |
| P-1 | Secure input and secure fields are refused | — | Closed by design |
| P-2 | Password-manager fields are never read | — | Closed by design |
| P-3 | No permission beyond the ones needed | — | Closed by design |
| P-4 | The block list is a hard stop | — | **Fixed** |
| S-1 | The usage log has no text | — | Closed by design |
| S-2 | The voice feedback log stores edit text | Low | Accepted, bounded and documented |
| S-3 | The undo record holds the field | Low | Accepted, documented |
| S-4 | The suggestion event log has no text | — | Closed by design |
| S-5 | The key is never in the config or the logs | — | Closed by design |
| T-1 | The read-back accepted a corrupted paste | **High** | **Fixed** |
| T-2 | The clipboard restore destroyed non-text items | **High** | **Fixed** |
| T-3 | The clipboard was cleared before the paste | **High** | **Fixed** |
| T-4 | The selection and the replacement are capped separately | — | Closed |
| P-1 | A user instruction could close its prompt wrapper | Medium | **Fixed** |
| P-2 | Document text is delimited and marked as data | — | Closed by design |
| P-3 | The model cannot choose where an edit lands | — | Closed by design |
| M-1 | The model could copy the writer's samples | Medium | **Fixed** |
| M-2 | The default request 400'd on the default model | **High** | **Fixed** |

### Findings by severity

- **High, fixed:** the clipboard data loss (T-2, T-3), the paste read-back
  (T-1), and the broken default request (M-2). All three were on a path a
  user reaches by pressing a shortcut.
- **Medium, fixed:** the prompt-wrapper escape (P-1) and the copy guard (M-1).
- **Medium, open:** the IPC frame size limit and protocol version (I-2). Both
  must land before the M3 browser bridge reuses this transport, because the
  bridge turns a local pipe into a network surface.
- **Low, accepted:** the bundle-identifier request (I-3), the voice feedback
  text (S-2), and the undo record (S-3). Each is a deliberate trade-off with a
  documented bound.

### What this review does not cover

- The **vendored Harper crates** (`vendor/harper/`) were not re-reviewed line
  by line. They are byte-identical to the pinned upstream commit apart from
  one documented `Cargo.toml` change, and the upstream audit is Harper's to
  own. The `cargo audit` CI job covers their dependency tree.
- The **Windows broker** (`apps/macos/src-tauri/src/windows_broker/`) is
  inherited scaffolding for a post-v1 platform and is not compiled into the
  macOS app.
- **Third-party model behaviour** is out of scope. The copy guard and the
  quote-resolution design reduce what a confused model can do; they cannot
  make a model's judgement correct.
