<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  type Permission = "Granted" | "NotGranted" | "Unsupported" | "Unknown";
  type KeyStatus = { present: boolean };
  type OpenRouterStatus = {
    label: string | null;
    usage: number | null;
    limit: number | null;
    is_free_tier: boolean;
  };
  type RewriteResult = {
    text: string;
    model: string;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number | null;
  };
  type UsageEntry = {
    timestamp: number;
    model: string;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number | null;
  };
  type UsageSummary = {
    requests: number;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number;
    recent: UsageEntry[];
  };
  type Suggestion = {
    id: string;
    span: {
      utf16_start: number;
      utf16_end: number;
      grapheme_start: number;
      grapheme_end: number;
    };
    original: string;
    replacement: string;
    category: string;
    severity: string;
    explanation: string;
    resolution: string;
  };
  type AnalyzeResponse = {
    suggestions: Suggestion[];
    rejected: { paragraph_id: string; original: string; reason: string }[];
    paragraphs: { id: string; text: string; byte_start: number; byte_end: number }[];
    model: string;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number | null;
  };

  let version = "0.1.0";
  let permission: Permission = "Unknown";
  let onboardingCompleted: boolean | null = null;
  let onboardingBusy = false;
  let onboardingMessage = "";
  let running = false;
  let busy = false;
  let message = "";

  let keyPresent = false;
  let keyInput = "";
  let model = "anthropic/claude-haiku-5.5";
  let modelBusy = false;
  let modelMessage = "";
  let usage: UsageSummary | null = null;
  let providerBusy = false;
  let providerMessage = "";
  let providerStatus: OpenRouterStatus | null = null;

  let sourceText =
    "Tehre are alot of ways to say this, but I think this one is more better.";
  let rewriteOutput = "";
  let rewriteMeta = "";

  let analysisText =
    "Their going to the conference next week , and they has a presentation.\n\nThis is definitely fine.";
  let analysisBusy = false;
  let analysisMessage = "";
  let analysisResult: AnalyzeResponse | null = null;

  const permissionLabel: Record<Permission, string> = {
    Granted: "Granted",
    NotGranted: "Not granted",
    Unsupported: "Unsupported on this system",
    Unknown: "Checking…",
  };

  async function refresh() {
    try {
      version = await invoke<string>("get_current_version");
      permission = await invoke<Permission>("get_accessibility_permission_status");
    } catch (error) {
      message = String(error);
    }
  }

  async function refreshKey() {
    try {
      const status = await invoke<KeyStatus>("get_openrouter_key_status");
      keyPresent = status.present;
    } catch (error) {
      providerMessage = String(error);
    }
  }

  async function refreshModel() {
    try {
      model = await invoke<string>("get_provider_model");
    } catch (error) {
      providerMessage = String(error);
    }
  }

  async function refreshUsage() {
    try {
      usage = await invoke<UsageSummary>("get_usage_summary", { limit: 5 });
    } catch (error) {
      providerMessage = String(error);
    }
  }

  async function saveModel() {
    modelBusy = true;
    modelMessage = "";
    try {
      model = await invoke<string>("set_provider_model", { model });
      modelMessage = "Model saved for Rewrite Selection and assistant requests.";
    } catch (error) {
      modelMessage = String(error);
    } finally {
      modelBusy = false;
    }
  }

  async function requestAccess() {
    busy = true;
    message = "";
    try {
      permission = await invoke<Permission>("request_accessibility_permission");
      message =
        permission === "Granted"
          ? "Accessibility is granted. Start suggestions below."
          : "macOS may show a permission prompt. If it does not, open System Settings → Privacy & Security → Accessibility and enable Elocutio.";
    } catch (error) {
      message = String(error);
    } finally {
      busy = false;
    }
  }

  async function start() {
    busy = true;
    message = "";
    try {
      running = await invoke<boolean>("start_highlighter_service");
      message = running
        ? "Suggestions are running. Focus a text field in another app."
        : "The suggestion service did not start. Check Accessibility.";
    } catch (error) {
      message = String(error);
    } finally {
      busy = false;
    }
  }

  async function stop() {
    busy = true;
    message = "";
    try {
      running = await invoke<boolean>("stop_highlighter_service");
      message = "Suggestions stopped.";
    } catch (error) {
      message = String(error);
    } finally {
      busy = false;
    }
  }

  async function saveKey() {
    providerBusy = true;
    providerMessage = "";
    try {
      const status = await invoke<KeyStatus>("set_openrouter_key", { key: keyInput });
      keyPresent = status.present;
      keyInput = "";
      providerMessage = "Key saved in the macOS Keychain.";
    } catch (error) {
      providerMessage = String(error);
    } finally {
      providerBusy = false;
    }
  }

  async function clearKey() {
    providerBusy = true;
    providerMessage = "";
    try {
      const status = await invoke<KeyStatus>("clear_openrouter_key");
      keyPresent = status.present;
      providerStatus = null;
      providerMessage = "Key removed.";
    } catch (error) {
      providerMessage = String(error);
    } finally {
      providerBusy = false;
    }
  }

  async function testKey() {
    providerBusy = true;
    providerMessage = "";
    try {
      providerStatus = await invoke<OpenRouterStatus>("test_openrouter_key");
      providerMessage = "OpenRouter accepted the key.";
    } catch (error) {
      providerMessage = String(error);
    } finally {
      providerBusy = false;
    }
  }

  async function analyze() {
    analysisBusy = true;
    analysisMessage = "";
    analysisResult = null;
    try {
      analysisResult = await invoke<AnalyzeResponse>("analyze_text", {
        text: analysisText,
      });
      analysisMessage = `Analyzed ${analysisResult.paragraphs.length} paragraph(s) with ${analysisResult.model}.`;
      void refreshUsage();
    } catch (error) {
      analysisMessage = String(error);
    } finally {
      analysisBusy = false;
    }
  }

  async function rewrite() {
    providerBusy = true;
    providerMessage = "";
    rewriteOutput = "";
    rewriteMeta = "";
    try {
      const result = await invoke<RewriteResult>("rewrite_text", {
        text: sourceText,
        model,
      });
      rewriteOutput = result.text;
      const cost =
        result.cost_usd === null ? "cost unavailable" : `$${result.cost_usd.toFixed(4)}`;
      rewriteMeta = `${result.model} · ${result.input_tokens} in / ${result.output_tokens} out · ${cost}`;
      void refreshUsage();
    } catch (error) {
      providerMessage = String(error);
    } finally {
      providerBusy = false;
    }
  }

  async function refreshOnboarding() {
    try {
      onboardingCompleted = await invoke<boolean>("get_onboarding_completed");
    } catch (error) {
      onboardingMessage = String(error);
    }
  }

  async function finishSetup() {
    onboardingBusy = true;
    onboardingMessage = "";
    try {
      await invoke("set_onboarding_completed", { onboardingCompleted: true });
      onboardingCompleted = true;
      onboardingMessage =
        "Setup complete. Elocutio stays in the menu bar; this window will not reopen automatically.";
    } catch (error) {
      onboardingMessage = String(error);
    } finally {
      onboardingBusy = false;
    }
  }

  onMount(() => {
    void refresh();
    void refreshKey();
    void refreshOnboarding();
    void refreshModel();
    void refreshUsage();
  });
</script>

<main>
  <header>
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="/lockup-white.png" />
      <img src="/lockup-indigo.png" alt="Elocutio" class="logo" />
    </picture>
    <p class="tagline">Your voice. Clearly expressed.</p>
  </header>

  {#if onboardingCompleted === false}
    <section class="card onboarding">
      <h2>Welcome to Elocutio</h2>
      <p class="muted">A private writing companion. Two quick checks and you are ready.</p>

      <ul class="checklist">
        <li class:done={permission === "Granted"}>
          <span class="check">{permission === "Granted" ? "✓" : "○"}</span>
          <div>
            <strong>Accessibility</strong>
            <p class="muted">
              {permission === "Granted"
                ? "Elocutio can read the focused text field."
                : "Required for suggestions and system-wide rewrite."}
            </p>
          </div>
          {#if permission !== "Granted"}
            <button class="small" onclick={requestAccess} disabled={busy}>Grant</button>
          {/if}
        </li>

        <li class:done={keyPresent}>
          <span class="check">{keyPresent ? "✓" : "○"}</span>
          <div>
            <strong>OpenRouter key</strong>
            <p class="muted">
              {keyPresent
                ? "Saved in the macOS Keychain."
                : "Optional. Add one below for model-backed rewrites; local checks work without it."}
            </p>
          </div>
        </li>

        <li class="done">
          <span class="check">✓</span>
          <div>
            <strong>Rewrite shortcut</strong>
            <p class="muted">Select text in another app and press <kbd>⌥⌘E</kbd>.</p>
          </div>
        </li>
      </ul>

      <div class="actions">
        <button class="primary" onclick={finishSetup} disabled={onboardingBusy}>
          Finish setup
        </button>
      </div>
      {#if onboardingMessage}
        <p class="message">{onboardingMessage}</p>
      {/if}
    </section>
  {/if}

  <section class="card">
    <div class="row">
      <div>
        <h2>Suggestions</h2>
        <p class="muted">
          Local correctness checks run on your Mac. Model-backed suggestions arrive in M1.
        </p>
      </div>
      <span class:on={running} class:off={!running} class="status">
        {running ? "Running" : "Stopped"}
      </span>
    </div>

    <dl>
      <div>
        <dt>Accessibility</dt>
        <dd class:ok={permission === "Granted"} class:warn={permission !== "Granted"}>
          {permissionLabel[permission]}
        </dd>
      </div>
      <div>
        <dt>Version</dt>
        <dd>{version}</dd>
      </div>
    </dl>

    <div class="actions">
      {#if permission !== "Granted"}
        <button class="primary" onclick={requestAccess} disabled={busy}>
          Grant Accessibility
        </button>
      {:else if !running}
        <button class="primary" onclick={start} disabled={busy}>Start suggestions</button>
      {:else}
        <button onclick={stop} disabled={busy}>Stop suggestions</button>
      {/if}
      <button class="ghost" onclick={refresh} disabled={busy}>Refresh</button>
    </div>

    {#if message}
      <p class="message">{message}</p>
    {/if}
  </section>

  <section class="card">
    <div class="row">
      <div>
        <h2>OpenRouter</h2>
        <p class="muted">Bring your own key. It is stored in the macOS Keychain, not in config.</p>
      </div>
      <span class="status" class:on={keyPresent} class:off={!keyPresent}>
        {keyPresent ? "Key saved" : "No key"}
      </span>
    </div>

    {#if !keyPresent}
      <label class="field">
        <span>API key</span>
        <input
          type="password"
          bind:value={keyInput}
          placeholder="sk-or-v1-…"
          autocomplete="off"
          spellcheck="false"
        />
      </label>
      <div class="actions">
        <button class="primary" onclick={saveKey} disabled={providerBusy || !keyInput.trim()}>
          Save to Keychain
        </button>
      </div>
    {:else}
      <div class="actions">
        <button onclick={testKey} disabled={providerBusy}>Test key</button>
        <button class="ghost" onclick={clearKey} disabled={providerBusy}>Remove key</button>
      </div>
    {/if}

    {#if providerStatus}
      <dl class="provider-details">
        <div>
          <dt>Label</dt>
          <dd>{providerStatus.label ?? "—"}</dd>
        </div>
        <div>
          <dt>Usage / limit</dt>
          <dd>
            ${providerStatus.usage?.toFixed(2) ?? "0.00"} /
            {providerStatus.limit === null ? "unlimited" : `$${providerStatus.limit.toFixed(2)}`}
          </dd>
        </div>
        <div>
          <dt>Tier</dt>
          <dd>{providerStatus.is_free_tier ? "Free" : "Paid"}</dd>
        </div>
      </dl>
    {/if}

    {#if providerMessage}
      <p class="message">{providerMessage}</p>
    {/if}
  </section>

  <section class="card">
    <h2>Test a rewrite</h2>
    <p class="muted">
      This proves the provider path in-app; system-wide model suggestions and span replacement follow
      in M1.
    </p>

    <label class="field">
      <span>Model</span>
      <div class="model-row">
        <input bind:value={model} spellcheck="false" />
        <button onclick={saveModel} disabled={modelBusy || !model.trim()}>Save</button>
      </div>
    </label>

    {#if modelMessage}
      <p class="message">{modelMessage}</p>
    {/if}

    <label class="field">
      <span>Text</span>
      <textarea bind:value={sourceText} rows="4"></textarea>
    </label>

    <div class="actions">
      <button class="primary" onclick={rewrite} disabled={providerBusy || !keyPresent}>
        Rewrite with OpenRouter
      </button>
    </div>

    {#if rewriteOutput}
      <div class="rewrite">
        <p class="rewrite-label">Result</p>
        <p class="rewrite-text">{rewriteOutput}</p>
        <p class="muted">{rewriteMeta}</p>
      </div>
    {/if}
  </section>

  <section class="card">
    <h2>Analyze a document</h2>
    <p class="muted">
      Structured proofreading: the model returns verbatim quotes, and Elocutio resolves them to
      validated UTF-16/grapheme spans locally. Ambiguous edits are dropped.
    </p>

    <label class="field">
      <span>Text</span>
      <textarea bind:value={analysisText} rows="5"></textarea>
    </label>

    <div class="actions">
      <button class="primary" onclick={analyze} disabled={analysisBusy || !keyPresent}>
        Analyze with OpenRouter
      </button>
    </div>

    {#if analysisMessage}
      <p class="message">{analysisMessage}</p>
    {/if}

    {#if analysisResult}
      <p class="muted">
        {analysisResult.suggestions.length} suggestion(s), {analysisResult.rejected.length}
        rejected · {analysisResult.input_tokens} in / {analysisResult.output_tokens} out
        {#if analysisResult.cost_usd !== null}
          · ${analysisResult.cost_usd.toFixed(4)}
        {/if}
      </p>

      {#if analysisResult.suggestions.length > 0}
        <ul class="suggestion-list">
          {#each analysisResult.suggestions as suggestion}
            <li>
              <div class="suggestion-head">
                <strong>{suggestion.category}</strong>
                <span class="badge">{suggestion.severity}</span>
                <span class="badge">{suggestion.resolution}</span>
                <span class="muted">{suggestion.id}</span>
              </div>
              <p class="suggestion-diff">
                <span class="remove">{suggestion.original}</span>
                →
                <span class="add">{suggestion.replacement}</span>
              </p>
              <p class="muted">{suggestion.explanation}</p>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="muted">No safe suggestions were produced.</p>
      {/if}
    {/if}
  </section>

  <section class="card">
    <div class="row">
      <div>
        <h2>Usage</h2>
        <p class="muted">Local metadata only. Text is never written to this log.</p>
      </div>
      <button class="ghost small" onclick={refreshUsage}>Refresh</button>
    </div>

    {#if usage}
      <dl class="provider-details">
        <div>
          <dt>Requests</dt>
          <dd>{usage.requests}</dd>
        </div>
        <div>
          <dt>Tokens</dt>
          <dd>{usage.input_tokens + usage.output_tokens}</dd>
        </div>
        <div>
          <dt>Cost</dt>
          <dd>${usage.cost_usd.toFixed(4)}</dd>
        </div>
      </dl>

      {#if usage.recent.length > 0}
        <ul class="usage-list">
          {#each usage.recent as entry}
            <li>
              <span>{new Date(entry.timestamp * 1000).toLocaleString()}</span>
              <span>{entry.model}</span>
              <span>${entry.cost_usd === null ? "—" : entry.cost_usd.toFixed(4)}</span>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="muted">No model requests recorded yet.</p>
      {/if}
    {:else}
      <p class="muted">Loading usage…</p>
    {/if}
  </section>

  <section class="card">
    <h2>Try the overlay</h2>
    <ol>
      <li>Open TextEdit, Notes, Mail, or a browser text field.</li>
      <li>Type a sentence with a deliberate typo, such as “this is tehre test”.</li>
      <li>Elocutio should underline the issue and show a small suggestion card.</li>
    </ol>
    <p class="muted">
      If nothing appears, confirm Accessibility is granted, then stop and start suggestions.
      Secure fields, terminals, and Google Docs are intentionally unsupported.
    </p>
  </section>

  <footer>
    <span>M0 developer shell</span>
    <a href="https://github.com/pri-te-sh/elocutio" target="_blank" rel="noreferrer">
      Source and roadmap
    </a>
  </footer>
</main>

<style>
  main {
    max-width: 720px;
    margin: 0 auto;
    padding: 48px 28px 32px;
  }

  header {
    margin-bottom: 28px;
  }

  picture {
    display: block;
  }

  .logo {
    display: block;
    width: 280px;
    max-width: 80%;
    height: auto;
  }

  .tagline {
    margin: 12px 0 0;
    color: var(--muted);
    font-size: 15px;
  }

  .card {
    background: var(--white);
    border: 1px solid var(--border);
    border-radius: 16px;
    padding: 20px 22px;
    margin-bottom: 18px;
  }

  .row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 18px;
  }

  h2 {
    margin: 0 0 6px;
    color: var(--accent);
    font-size: 17px;
    font-weight: 600;
  }

  p,
  li {
    line-height: 1.55;
  }

  .muted {
    margin: 0;
    color: var(--muted);
    font-size: 13px;
  }

  kbd {
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 5px;
    padding: 1px 5px;
    font: inherit;
    font-size: 12px;
    background: var(--warm-paper);
    color: var(--near-black);
  }

  .status {
    flex: none;
    border-radius: 999px;
    padding: 4px 10px;
    font-size: 12px;
    font-weight: 600;
    background: rgba(154, 91, 0, 0.12);
    color: var(--warning);
  }

  .status.on {
    background: rgba(31, 122, 77, 0.12);
    color: var(--success);
  }

  .status.off {
    background: rgba(154, 91, 0, 0.12);
    color: var(--warning);
  }

  dl {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
    margin: 18px 0;
  }

  .provider-details {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  dl > div {
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }

  dt {
    color: var(--muted);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  dd {
    margin: 4px 0 0;
    font-size: 14px;
    font-weight: 600;
  }

  dd.ok {
    color: var(--success);
  }

  dd.warn {
    color: var(--warning);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  button {
    border-radius: 10px;
    border: 1px solid var(--border);
    padding: 9px 14px;
    cursor: pointer;
    background: transparent;
    color: var(--near-black);
    font-weight: 600;
    font-size: 13px;
  }

  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
  }

  button.ghost {
    background: transparent;
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .message {
    margin: 14px 0 0;
    padding: 10px 12px;
    border-radius: 10px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 13px;
  }

  .field {
    display: block;
    margin: 14px 0;
  }

  .model-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .model-row input {
    flex: 1;
  }

  .suggestion-list {
    list-style: none;
    margin: 14px 0 0;
    padding: 0;
    display: grid;
    gap: 12px;
  }

  .suggestion-list li {
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }

  .suggestion-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: 12px;
  }

  .suggestion-diff {
    margin: 6px 0 4px;
    font-size: 14px;
    line-height: 1.5;
  }

  .suggestion-diff .remove {
    text-decoration: line-through;
    color: var(--muted);
  }

  .suggestion-diff .add {
    color: var(--success);
    font-weight: 600;
  }

  .badge {
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px 7px;
    color: var(--muted);
    font-size: 11px;
  }

  .usage-list {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .usage-list li {
    display: grid;
    grid-template-columns: 1.2fr 1.4fr auto;
    gap: 8px;
    border-top: 1px solid var(--border);
    padding-top: 6px;
    color: var(--muted);
    font-size: 12px;
  }

  .field span {
    display: block;
    margin-bottom: 6px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  input,
  textarea {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 9px 11px;
    font: inherit;
    font-size: 13px;
    color: var(--near-black);
    background: var(--warm-paper);
  }

  textarea {
    resize: vertical;
    line-height: 1.5;
  }

  .rewrite {
    margin-top: 14px;
    border-top: 1px solid var(--border);
    padding-top: 12px;
  }

  .rewrite-label {
    margin: 0 0 4px;
    color: var(--muted);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .rewrite-text {
    margin: 0 0 6px;
    color: var(--near-black);
    font-size: 15px;
  }

  ol {
    margin: 10px 0;
    padding-left: 22px;
  }

  .onboarding {
    border-color: color-mix(in srgb, var(--accent) 30%, var(--border));
  }

  .checklist {
    list-style: none;
    margin: 16px 0;
    padding: 0;
    display: grid;
    gap: 12px;
  }

  .checklist li {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    border-top: 1px solid var(--border);
    padding-top: 12px;
  }

  .checklist li:first-child {
    border-top: 0;
    padding-top: 0;
  }

  .checklist .check {
    flex: none;
    width: 20px;
    color: var(--muted);
    font-weight: 700;
  }

  .checklist li.done .check {
    color: var(--success);
  }

  .checklist strong {
    display: block;
    color: var(--near-black);
    font-size: 14px;
  }

  .checklist p {
    margin: 2px 0 0;
  }

  button.small {
    margin-left: auto;
    flex: none;
    padding: 5px 10px;
    font-size: 12px;
  }

  footer {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    margin-top: 28px;
    color: var(--muted);
    font-size: 12px;
  }

  footer a {
    text-decoration: none;
  }
</style>
