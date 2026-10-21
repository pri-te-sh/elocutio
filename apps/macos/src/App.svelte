<script lang="ts">
  /**
   * Elocutio — the macOS settings window.
   *
   * This file is the only UI the app has, so every backend command a user
   * could reasonably want has to be reachable from here. The commands this
   * file deliberately does *not* call are listed in `SKIPPED_COMMANDS` below,
   * with the reason for each.
   */
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  /**
   * The backend commands this file deliberately does not call, and why.
   *
   * A command that is neither called nor explained is the defect this pass
   * exists to fix, so every deliberate omission is written down here.
   *
   * - `evaluate_golden_set` — a library-facing API. It takes a golden set
   *   and a prediction set as JSON, and the app has no way to produce a
   *   prediction set to feed it. The resolver half of the same harness is
   *   reachable through `run_resolver_quality_check`, which runs against the
   *   seed set the app already ships, so the number a user can actually see
   *   is the one that matters. Plumbing 19 KB of golden JSON through IPC to
   *   call the other half would add a way to get the same number wrong.
   * - `merge_suggestions` — a library-facing API. It merges a local
   *   suggestion list with an LLM suggestion list, and the settings window
   *   has no local suggestion list and no second LLM list to merge with.
   * - `set_voice_trait_override` — duplicates `set_voice_trait_value`, which
   *   the UI already called. Both take the same untagged number|bool|string
   *   and set the same override; `set_voice_trait_value` picks the trait kind
   *   from the value, so one command in the UI is enough.
   */
  const SKIPPED_COMMANDS: readonly { command: string; reason: string }[] = [
    {
      command: "evaluate_golden_set",
      reason:
        "Library-facing API: needs a prediction set the app cannot produce. " +
        "run_resolver_quality_check covers the reachable half of the same harness.",
    },
    {
      command: "merge_suggestions",
      reason:
        "Library-facing API: merges a local and an LLM suggestion list, and the " +
        "settings window has neither.",
    },
    {
      command: "set_voice_trait_override",
      reason:
        "Duplicates set_voice_trait_value, which the UI already used. Only one " +
        "command should do this job.",
    },
  ];

  type Permission = "Granted" | "NotGranted" | "Unsupported" | "Unknown";
  type KeyStatus = { present: boolean };
  type OpenRouterStatus = {
    label: string | null;
    usage: number | null;
    limit: number | null;
    is_free_tier: boolean;
  };

  type DiffSegment = { kind: "equal" | "insert" | "delete"; text: string };
  type RewritePreview = {
    original: string;
    rewritten: string;
    diff: DiffSegment[];
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
  type RewriteTone = "neutral" | "confident" | "friendly" | "concise" | "formal";
  type ProviderSettings = {
    kind: "open_router" | "open_ai_compatible";
    base_url: string;
    model: string;
    key_present: boolean;
  };
  type IntegrationView = {
    display_name: string;
    bundle_id: string;
    enabled: boolean;
  };
  type VoiceTrait = {
    value: number | boolean | string;
    confidence: number;
    evidence: string[];
  };
  type VoiceProfile = {
    id: string;
    name: string;
    context: string;
    language: string;
    strength: string;
    confidence: number;
    summary: string;
    traits: Record<string, VoiceTrait>;
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
  type EventSummary = {
    accepted: number;
    dismissed: number;
    edited_after_accept: number;
    rated_positive: number;
    rated_negative: number;
    by_category: Record<string, number>;
    dismissed_by_category: Record<string, number>;
  };
  type ToneResponse = {
    tone: "neutral" | "confident" | "friendly" | "concise" | "formal";
    label: string;
    confidence: number;
    evidence: string[];
    guidance: string | null;
  };
  type VoiceTraitView = {
    key: string;
    value: number | boolean | string;
    confidence: number;
    is_override: boolean;
    evidence: string[];
  };
  type GateCheck = {
    id: string;
    description: string;
    state: "pass" | "fail" | "needs_human";
    detail: string;
  };
  type GateReport = {
    gate: string;
    checks: GateCheck[];
    passed: number;
    failed: number;
    needs_human: number;
  };
  type NetworkEntry = {
    at: number;
    host: string;
    provider: string;
    model: string | null;
    kind: string;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number | null;
    ok: boolean;
    status: number | null;
    error: string | null;
  };
  type NetworkSummary = {
    requests: number;
    succeeded: number;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number;
    hosts: [string, number][];
  };
  type MatrixApp = { bundle_id: string; name: string; tier: string };
  type MatrixCondition = { name: string; label: string };
  type MatrixSummary = {
    tier_a: number;
    supported: number;
    checked: number;
    failed: number;
    unverified: number;
    failing_apps: string[];
  };
  type RequestPreview = {
    host: string;
    provider: string;
    model: string | null;
    fields: { name: string; value: string; redacted: boolean }[];
    text: string;
    system: string;
  };
  type WidgetAnchor = { corner: string; offset: number };
  type ModelOption = {
    id: string;
    display_name: string;
    provider: string;
    model_id: string;
    context_window: number;
    price_input_per_million_usd: number;
    price_output_per_million_usd: number;
    best_for: string;
    supports_temperature: boolean;
    is_default: boolean;
  };

  // ---- Types for the commands that had no UI caller until now. ----

  /** `get_voice_profile_summary` — the stored profile, counted rather than spelled out. */
  type VoiceProfileSummary = {
    id: string;
    name: string;
    strength: string;
    confidence: number;
    summary: string;
    trait_count: number;
    override_count: number;
    feedback_count: number;
    sample_guards: number;
    learning_enabled: boolean;
  };
  /**
   * A trait value in the tagged shape the crate stores: the kind, then the
   * value. `get_voice_traits` returns the untagged shape instead.
   */
  type TaggedTraitValue = {
    kind: "number" | "bool" | "text";
    value: number | boolean | string;
  };
  /**
   * `get_voice_trait_list` — the trait with the reason it was learned. The
   * value is the tagged form, so the evidence is what this call is for: the
   * trait list below shows *why* a trait holds the value it holds.
   */
  type VoiceTraitEntry = {
    key: string;
    value: TaggedTraitValue;
    confidence: number;
    is_override: boolean;
    evidence: string[];
  };
  /** `get_voice_trait_overrides` — only the traits the user set by hand. */
  type VoiceTraitOverride = [string, TaggedTraitValue];
  /** `get_copy_guard_report` — which sample n-grams a candidate rewrite shares. */
  type CopyGuardReport = {
    overlaps: {
      ngram: string;
      fingerprint: number;
      candidate_start: number;
      candidate_end: number;
      sample_index?: number;
    }[];
    clean: boolean;
  };
  /** `rewrite_text` / `rewrite_text_streamed` — the raw provider answer. */
  type RewriteResult = {
    text: string;
    model: string;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number | null;
  };
  /** `get_pending_rewrite` and friends — a prepared rewrite, not yet applied. */
  type PendingRewrite = {
    original: string;
    rewritten: string;
    diff: DiffSegment[];
    model: string;
    created_at: number;
  };
  /** `run_resolver_quality_check` — the resolver scored against the seed golden set. */
  type ResolverQuality = {
    version: string;
    quotes: number;
    resolved: number;
    ambiguous: number;
    wrong: number;
    resolution_rate: number;
    ambiguous_rate: number;
    failures: string[];
  };
  /** `get_app_matrix` — the apps, the conditions, and what is not supported. */
  type AppMatrix = {
    tierA: MatrixApp[];
    tierB: MatrixApp[];
    unsupported: { bundleId: string; reason: string }[];
    conditions: MatrixCondition[];
  };
  /** The state of one matrix check. */
  type MatrixState = "pass" | "fail" | "unverified";
  /** `get_coalescer_state` — how many analyses the coalescer is tracking. */
  type CoalescerState = { tracked: number; empty: boolean };

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
  let providerKind: ProviderSettings["kind"] = "open_router";
  let providerBaseUrl = "https://api.openai.com/v1";
  let providerBaseInput = "";
  let providerSettingsBusy = false;
  let providerSettingsMessage = "";
  let rewriteTone: RewriteTone = "neutral";
  let toneBusy = false;
  let toneMessage = "";
  let localOnly = false;
  let localOnlyBusy = false;
  let localOnlyMessage = "";
  let autoAnalyze = false;
  let autoAnalysisBusy = false;
  let autoAnalysisMessage = "";
  let integrations: IntegrationView[] = [];
  let autoEnableNewApps = false;
  let appsBusy = false;
  let appsMessage = "";
  let budgetUsd: number | null = null;
  let budgetInput = "";
  let budgetBusy = false;
  let budgetMessage = "";
  let undoBusy = false;
  let undoMessage = "";
  let voiceProfile: VoiceProfile | null = null;
  let voiceSamples = "";
  let voiceBusy = false;
  let voiceMessage = "";
  let learnFromEdits = false;
  let voiceFeedbackCount = 0;
  let voiceGuidance: string | null = null;
  let eventsEnabled = false;
  let eventSummary: EventSummary | null = null;
  let eventsBusy = false;
  let eventsMessage = "";
  let toneResult: ToneResponse | null = null;
  let detectToneBusy = false;
  let blocklist: string[] = [];
  let voiceTraits: VoiceTraitView[] = [];
  let voiceRatingBusy = false;
  let modelOptions: ModelOption[] = [];
  let costCapUsd: number | null = null;
  let costCapInput = "";
  let costCapBusy = false;
  let costCapMessage = "";
  let gateReport: GateReport | null = null;
  let widgetAnchors: [string, WidgetAnchor][] = [];
  let networkLog: NetworkEntry[] = [];
  let requestPreview: RequestPreview | null = null;
  let matrixApps: MatrixApp[] = [];
  let matrixConditions: MatrixCondition[] = [];
  let matrixSummary: MatrixSummary | null = null;
  let previewBusy = false;
  let previewMessage = "";
  let networkSummary: NetworkSummary | null = null;
  let anchorCorners: string[] = [];
  let customInstruction = "";
  let instructionBusy = false;
  let instructionMessage = "";

  // Voice diagnostics: the counts and the guard report behind the voice card.
  let voiceSummary: VoiceProfileSummary | null = null;
  let voiceTraitList: VoiceTraitEntry[] = [];
  let voiceTraitOverrides: VoiceTraitOverride[] = [];
  let voiceEventCount = 0;
  let feedbackFingerprintCount = 0;
  let copyGuardReport: CopyGuardReport | null = null;
  let copyGuardText = "";
  let copyGuardBusy = false;
  let voiceContext = "";
  let recordedRating: string | null = null;

  // The rewrite flow: the raw provider answer, and the prepared system-wide
  // rewrite that the ⌥⌘E shortcut leaves behind.
  let rawRewrite: RewriteResult | null = null;
  let rawRewriteBusy = false;
  let streamedRewrite: RewriteResult | null = null;
  let pendingRewrite: PendingRewrite | null = null;
  let pendingBusy = false;
  let pendingMessage = "";

  // The resolver quality check against the seed golden set.
  let resolverQuality: ResolverQuality | null = null;
  let qualityBusy = false;
  let qualityMessage = "";

  // The app matrix: the apps, the conditions, and what has been checked.
  let matrixTierB: MatrixApp[] = [];
  let matrixUnsupported: { bundleId: string; reason: string }[] = [];
  let matrixChecks: Record<string, Record<string, MatrixState>> = {};
  let matrixBusy = false;
  let matrixMessage = "";
  let matrixOpenApp: string | null = null;

  // Diagnostics: the provider's default model and the request coalescer.
  let defaultModel = "";
  let coalescerState: CoalescerState | null = null;
  let coalescerBusy = false;

  // The active section in the table of contents.
  let activeSection = "general";

  let sourceText = "Tehre are alot of ways to say this, but I think this one is more better.";
  let rewriteOutput = "";
  let rewriteMeta = "";
  let rewriteDiff: DiffSegment[] = [];

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

  /**
   * The settings sections, in the order they appear.
   *
   * The table of contents is built from this list, so a section can never
   * appear in the nav without appearing in the page, and vice versa.
   */
  const sections = [
    { id: "general", label: "General" },
    { id: "provider", label: "Provider" },
    { id: "writing", label: "Writing" },
    { id: "voice", label: "Voice" },
    { id: "privacy", label: "Privacy" },
    { id: "quality", label: "Quality" },
    { id: "diagnostics", label: "Diagnostics" },
  ];

  /** The states a matrix check can be in, in the order they are offered. */
  const matrixStates: MatrixState[] = ["pass", "fail", "unverified"];

  /**
   * Jump to a section and remember it as the active one.
   *
   * The default anchor behaviour is kept — the URL hash updates, so the link
   * can be copied — but the scroll is smooth and the active link is tracked
   * here as well.
   */
  function jumpToSection(event: MouseEvent, id: string) {
    event.preventDefault();
    activeSection = id;
    const target = document.getElementById(id);
    if (!target) {
      return;
    }
    target.scrollIntoView({ behavior: "smooth", block: "start" });
    if (typeof history !== "undefined" && "replaceState" in history) {
      history.replaceState(null, "", `#${id}`);
    }
  }

  /**
   * Whether a section is the one on screen, used to keep the nav link in step.
   *
   * The line is the bottom edge of the sticky table of contents, so a section
   * counts as current once its heading has scrolled up past the nav.
   */
  function sectionIsVisible(id: string, line: number): boolean {
    const target = document.getElementById(id);
    if (!target) {
      return false;
    }
    const box = target.getBoundingClientRect();
    return box.top <= line && box.bottom > line;
  }

  /** Move the active link to whichever section is currently on screen. */
  function syncActiveSection() {
    const nav = document.querySelector(".toc");
    const line = (nav?.getBoundingClientRect().bottom ?? 0) + 8;
    for (const section of sections) {
      if (sectionIsVisible(section.id, line)) {
        activeSection = section.id;
        return;
      }
    }
  }

  async function refresh() {
    try {
      version = await invoke<string>("get_current_version");
      permission = await invoke<Permission>("get_accessibility_permission_status");
    } catch (error) {
      message = String(error);
    }
  }

  async function refreshLocalOnly() {
    try {
      localOnly = await invoke<boolean>("get_local_only");
    } catch (error) {
      localOnlyMessage = String(error);
    }
  }

  async function saveLocalOnly() {
    localOnlyBusy = true;
    localOnlyMessage = "";
    try {
      localOnly = await invoke<boolean>("set_local_only", { enabled: localOnly });
      localOnlyMessage = localOnly
        ? "Local-only mode is on. Provider requests are blocked."
        : "Providers are allowed again.";
    } catch (error) {
      localOnlyMessage = String(error);
    } finally {
      localOnlyBusy = false;
    }
  }

  async function refreshTone() {
    try {
      rewriteTone = await invoke<RewriteTone>("get_rewrite_tone");
    } catch (error) {
      toneMessage = String(error);
    }
  }

  async function saveTone() {
    toneBusy = true;
    toneMessage = "";
    try {
      rewriteTone = await invoke<RewriteTone>("set_rewrite_tone", { tone: rewriteTone });
      toneMessage = `Rewrite tone set to ${rewriteTone}.`;
    } catch (error) {
      toneMessage = String(error);
    } finally {
      toneBusy = false;
    }
  }

  async function refreshProvider() {
    try {
      const settings = await invoke<ProviderSettings>("get_provider_settings");
      providerKind = settings.kind;
      providerBaseUrl = settings.base_url;
      providerBaseInput = settings.base_url;
      model = settings.model;
      keyPresent = settings.key_present;
    } catch (error) {
      providerMessage = String(error);
    }
  }

  async function setProviderKind(kind: ProviderSettings["kind"]) {
    providerSettingsBusy = true;
    providerSettingsMessage = "";
    try {
      const settings = await invoke<ProviderSettings>("set_provider_kind", { kind });
      providerKind = settings.kind;
      providerBaseUrl = settings.base_url;
      providerBaseInput = settings.base_url;
      keyPresent = settings.key_present;
      providerStatus = null;
      providerSettingsMessage = `${kind === "open_router" ? "OpenRouter" : "OpenAI-compatible endpoint"} selected.`;
    } catch (error) {
      providerSettingsMessage = String(error);
    } finally {
      providerSettingsBusy = false;
    }
  }

  async function saveProviderBaseUrl() {
    providerSettingsBusy = true;
    providerSettingsMessage = "";
    try {
      providerBaseUrl = await invoke<string>("set_provider_base_url", {
        baseUrl: providerBaseInput,
      });
      providerBaseInput = providerBaseUrl;
      providerSettingsMessage = "Base URL saved.";
    } catch (error) {
      providerSettingsMessage = String(error);
    } finally {
      providerSettingsBusy = false;
    }
  }

  async function refreshModel() {
    try {
      model = await invoke<string>("get_provider_model");
    } catch (error) {
      providerMessage = String(error);
    }
  }

  async function refreshVoice() {
    try {
      voiceProfile = await invoke<VoiceProfile | null>("get_voice_profile");
      learnFromEdits = await invoke<boolean>("get_learn_from_edits");
      voiceFeedbackCount = await invoke<number>("get_voice_feedback_count");
      voiceGuidance = await invoke<string | null>("get_voice_guidance");
    } catch (error) {
      voiceMessage = String(error);
    }
  }

  async function setLearnFromEdits(enabled: boolean) {
    voiceBusy = true;
    voiceMessage = "";
    try {
      learnFromEdits = await invoke<boolean>("set_learn_from_edits", { enabled });
      await refreshVoice();
      voiceMessage = enabled
        ? "Elocutio will learn from accepted edits on this Mac."
        : "Edit learning is off.";
    } catch (error) {
      voiceMessage = String(error);
    } finally {
      voiceBusy = false;
    }
  }

  async function loadVoiceTraits() {
    try {
      voiceTraits = await invoke<VoiceTraitView[]>("get_voice_traits");
    } catch (error) {
      voiceMessage = String(error);
    }
  }

  async function rateVoice(rating: "thumbs_up" | "thumbs_down") {
    voiceRatingBusy = true;
    try {
      voiceProfile = await invoke<VoiceProfile | null>("rate_voice_rewrite", { rating });
      await loadVoiceTraits();
      voiceMessage =
        rating === "thumbs_up"
          ? "Thanks — the profile is a little more confident now."
          : "Noted — the profile is a little less confident now.";
    } catch (error) {
      voiceMessage = String(error);
    } finally {
      voiceRatingBusy = false;
    }
  }

  async function clearTraitOverride(key: string) {
    try {
      voiceProfile = await invoke<VoiceProfile | null>("clear_voice_trait_override", { key });
      await loadVoiceTraits();
      voiceMessage = `Back to what Elocutio learned for ${key}.`;
    } catch (error) {
      voiceMessage = String(error);
    }
  }

  // ---- Voice diagnostics ----------------------------------------------------
  //
  // The profile card above shows the profile the app is using right now.
  // These commands show the counts and the evidence behind it: how many
  // overrides there are, how many events have been recorded, and which
  // sample n-grams the copy guard is watching for.

  /**
   * The value typed into a trait's box, kept per trait so a half-typed value
   * is not thrown away by a refresh.
   */
  let traitDrafts: Record<string, string> = {};

  function traitDraft(key: string, fallback: number | boolean | string): string {
    return traitDrafts[key] ?? String(fallback);
  }

  function setTraitDraft(key: string, value: string) {
    traitDrafts = { ...traitDrafts, [key]: value };
  }

  /**
   * Save a trait the user typed.
   *
   * `set_voice_trait_value` takes the plain value and picks the trait kind
   * for you, so a number, `true`/`false`, and text all work from one box.
   *
   * The backend also exposes `set_voice_trait_override`, which takes the
   * same untagged number|bool|string and does the same thing. It is
   * deliberately not called from here: two commands doing one job in the UI
   * is the exact defect this pass is fixing, so the one that picks the kind
   * for the user wins.
   */
  async function saveTrait(key: string, raw: string) {
    const text = raw.trim();
    if (text === "") {
      voiceMessage = "Type a value before saving the trait.";
      return;
    }
    voiceBusy = true;
    voiceMessage = "";
    try {
      const value: number | boolean | string =
        text === "true"
          ? true
          : text === "false"
            ? false
            : Number.isNaN(Number(text))
              ? text
              : Number(text);
      voiceProfile = await invoke<VoiceProfile | null>("set_voice_trait_value", { key, value });
      await loadVoiceTraits();
      await refreshVoiceTraitDetails();
      await refreshVoiceSummary();
      voiceMessage = `Saved your value for ${key}.`;
    } catch (error) {
      voiceMessage = String(error);
    } finally {
      voiceBusy = false;
    }
  }

  /** Show a tagged trait value as plain text. */
  function taggedValueText(value: TaggedTraitValue): string {
    return typeof value.value === "string" ? value.value : String(value.value);
  }

  async function refreshVoiceSummary() {
    try {
      voiceSummary = await invoke<VoiceProfileSummary | null>("get_voice_profile_summary");
    } catch (error) {
      voiceMessage = String(error);
    }
  }

  async function refreshVoiceTraitDetails() {
    try {
      voiceTraitList = await invoke<VoiceTraitEntry[]>("get_voice_trait_list");
      voiceTraitOverrides = await invoke<VoiceTraitOverride[]>("get_voice_trait_overrides");
    } catch (error) {
      voiceMessage = String(error);
    }
  }
  async function refreshVoiceCounts() {
    try {
      voiceEventCount = await invoke<number>("get_voice_event_count");
      feedbackFingerprintCount = await invoke<number>("get_feedback_fingerprint_count");
    } catch (error) {
      voiceMessage = String(error);
    }
  }

  async function checkCopyGuard() {
    copyGuardBusy = true;
    copyGuardReport = null;
    try {
      copyGuardReport = await invoke<CopyGuardReport | null>("get_copy_guard_report", {
        candidate: copyGuardText,
      });
    } catch (error) {
      voiceMessage = String(error);
    } finally {
      copyGuardBusy = false;
    }
  }

  /**
   * Record a rating for one specific piece of writing.
   *
   * `rate_voice_rewrite` (used by the "Does this sound like you?" buttons
   * above) rates the *last rewrite* and folds the rating into the profile.
   * This records a rating about something else — a reply you wrote, say —
   * without changing the profile, and keeps it next to the other feedback.
   */
  async function recordVoiceRating(rating: "thumbs_up" | "thumbs_down") {
    voiceRatingBusy = true;
    voiceMessage = "";
    try {
      await invoke("record_voice_rating", {
        rating,
        context: voiceContext.trim() === "" ? null : voiceContext.trim(),
      });
      recordedRating = rating;
      voiceMessage = "Rating recorded. It is stored locally with your other feedback.";
      await refreshVoiceCounts();
    } catch (error) {
      voiceMessage = String(error);
    } finally {
      voiceRatingBusy = false;
    }
  }

  async function refreshModels() {
    try {
      modelOptions = await invoke<ModelOption[]>("list_provider_models");
    } catch (error) {
      providerMessage = String(error);
    }
  }

  async function refreshCostCap() {
    try {
      costCapUsd = await invoke<number | null>("get_cost_cap");
      costCapInput = costCapUsd === null ? "" : String(costCapUsd);
    } catch (error) {
      costCapMessage = String(error);
    }
  }

  async function saveCostCap() {
    costCapBusy = true;
    costCapMessage = "";
    try {
      const parsed = costCapInput.trim() === "" ? null : Number(costCapInput);
      costCapUsd = await invoke<number | null>("set_cost_cap", { costCapUsd: parsed });
      costCapInput = costCapUsd === null ? "" : String(costCapUsd);
      costCapMessage =
        costCapUsd === null
          ? "Cost cap cleared."
          : `Requests over $${costCapUsd.toFixed(2)} are refused before they are sent.`;
    } catch (error) {
      costCapMessage = String(error);
    } finally {
      costCapBusy = false;
    }
  }

  async function refreshMatrix() {
    try {
      const matrix = await invoke<AppMatrix>("get_app_matrix");
      matrixApps = matrix.tierA;
      matrixTierB = matrix.tierB;
      matrixUnsupported = matrix.unsupported;
      matrixConditions = matrix.conditions;
      matrixSummary = await invoke<MatrixSummary>("get_app_matrix_summary");
    } catch (error) {
      matrixMessage = String(error);
    }
  }

  async function previewRequest() {
    previewBusy = true;
    previewMessage = "";
    try {
      requestPreview = await invoke<RequestPreview>("preview_request", { text: sourceText });
    } catch (error) {
      previewMessage = String(error);
    } finally {
      previewBusy = false;
    }
  }

  async function refreshNetworkLog() {
    try {
      networkLog = await invoke<NetworkEntry[]>("get_network_log");
      networkSummary = await invoke<NetworkSummary>("get_network_log_summary");
    } catch (error) {
      instructionMessage = String(error);
    }
  }

  async function clearNetworkLog() {
    try {
      networkSummary = await invoke<NetworkSummary>("clear_network_log");
      networkLog = [];
    } catch (error) {
      instructionMessage = String(error);
    }
  }

  async function refreshWidgetAnchors() {
    try {
      widgetAnchors = await invoke<[string, WidgetAnchor][]>("get_widget_anchors");
      anchorCorners = await invoke<string[]>("get_widget_anchor_corners");
    } catch (error) {
      instructionMessage = String(error);
    }
  }

  async function setWidgetAnchor(bundleId: string, corner: string | null) {
    try {
      const anchor = corner === null ? null : { corner, offset: 24 };
      widgetAnchors = await invoke<[string, WidgetAnchor][]>("set_widget_anchor", {
        bundleId,
        anchor,
      });
    } catch (error) {
      instructionMessage = String(error);
    }
  }

  async function refreshGate() {
    try {
      gateReport = await invoke<GateReport>("run_m1_gate");
    } catch (error) {
      instructionMessage = String(error);
    }
  }

  async function refreshInstruction() {
    try {
      customInstruction = await invoke<string>("get_custom_instruction");
    } catch (error) {
      instructionMessage = String(error);
    }
  }

  async function saveInstruction() {
    instructionBusy = true;
    instructionMessage = "";
    try {
      customInstruction = await invoke<string>("set_custom_instruction", {
        instruction: customInstruction,
      });
      instructionMessage = "Saved. This instruction is applied to every request.";
    } catch (error) {
      instructionMessage = String(error);
    } finally {
      instructionBusy = false;
    }
  }

  async function refreshEvents() {
    try {
      eventsEnabled = await invoke<boolean>("get_suggestion_events_enabled");
      eventSummary = await invoke<EventSummary>("get_suggestion_event_summary");
    } catch (error) {
      eventsMessage = String(error);
    }
  }

  async function setEventsEnabled(enabled: boolean) {
    eventsBusy = true;
    eventsMessage = "";
    try {
      eventsEnabled = await invoke<boolean>("set_suggestion_events_enabled", { enabled });
      eventSummary = await invoke<EventSummary>("get_suggestion_event_summary");
      eventsMessage = enabled
        ? "Elocutio will record which suggestions you accept and dismiss."
        : "Suggestion events are off.";
    } catch (error) {
      eventsMessage = String(error);
    } finally {
      eventsBusy = false;
    }
  }

  async function clearEvents() {
    eventsBusy = true;
    eventsMessage = "";
    try {
      eventSummary = await invoke<EventSummary>("clear_suggestion_events");
      eventsMessage = "Recorded suggestion events deleted.";
    } catch (error) {
      eventsMessage = String(error);
    } finally {
      eventsBusy = false;
    }
  }

  async function learnVoice() {
    voiceBusy = true;
    voiceMessage = "";
    try {
      voiceProfile = await invoke<VoiceProfile>("learn_voice_samples", {
        text: voiceSamples,
        name: "My voice",
      });
      voiceFeedbackCount = await invoke<number>("get_voice_feedback_count");
      voiceGuidance = await invoke<string | null>("get_voice_guidance");
      voiceMessage = "Voice learned. Only the style card is stored; the samples were discarded.";
    } catch (error) {
      voiceMessage = String(error);
    } finally {
      voiceBusy = false;
    }
  }

  async function setVoiceEnabled(enabled: boolean) {
    voiceBusy = true;
    voiceMessage = "";
    try {
      voiceProfile = await invoke<VoiceProfile | null>("set_voice_enabled", { enabled });
      voiceGuidance = await invoke<string | null>("get_voice_guidance");
      voiceMessage = enabled ? "Voice guidance is on for rewrites." : "Voice guidance is off.";
    } catch (error) {
      voiceMessage = String(error);
    } finally {
      voiceBusy = false;
    }
  }

  async function clearVoice() {
    voiceBusy = true;
    voiceMessage = "";
    try {
      await invoke("clear_voice_profile");
      voiceProfile = null;
      learnFromEdits = false;
      voiceFeedbackCount = 0;
      voiceGuidance = null;
      voiceMessage = "Voice profile and local feedback deleted.";
    } catch (error) {
      voiceMessage = String(error);
    } finally {
      voiceBusy = false;
    }
  }

  async function undoLastEdit() {
    undoBusy = true;
    undoMessage = "";
    try {
      undoMessage = await invoke<string>("undo_last_edit");
    } catch (error) {
      undoMessage = String(error);
    } finally {
      undoBusy = false;
    }
  }

  async function refreshBudget() {
    try {
      budgetUsd = await invoke<number | null>("get_budget");
      budgetInput = budgetUsd === null ? "" : String(budgetUsd);
    } catch (error) {
      budgetMessage = String(error);
    }
  }

  async function saveBudget() {
    budgetBusy = true;
    budgetMessage = "";
    try {
      const parsed = budgetInput.trim() === "" ? null : Number(budgetInput);
      if (parsed !== null && (!Number.isFinite(parsed) || parsed < 0)) {
        throw new Error("Enter a non-negative USD budget, or leave it blank.");
      }
      budgetUsd = await invoke<number | null>("set_budget", { budgetUsd: parsed });
      budgetInput = budgetUsd === null ? "" : String(budgetUsd);
      budgetMessage = budgetUsd === null ? "Budget warning removed." : "Budget warning saved.";
    } catch (error) {
      budgetMessage = String(error);
    } finally {
      budgetBusy = false;
    }
  }

  async function refreshIntegrations() {
    try {
      integrations = await invoke<IntegrationView[]>("get_integrations");
      autoEnableNewApps = await invoke<boolean>("get_auto_enable_new_apps");
      blocklist = await invoke<string[]>("get_blocklist");
    } catch (error) {
      appsMessage = String(error);
    }
  }

  async function setBlocked(bundleId: string, blocked: boolean) {
    appsBusy = true;
    appsMessage = "";
    try {
      blocklist = await invoke<string[]>("set_blocked", { bundleId, blocked });
      // Blocking an app also disables it as an integration, so refresh the
      // list to show the real state rather than a stale one.
      await refreshIntegrations();
      appsMessage = blocked
        ? "The app is blocked. Elocutio will not read it."
        : "The app is no longer blocked.";
    } catch (error) {
      appsMessage = String(error);
    } finally {
      appsBusy = false;
    }
  }

  async function setIntegrationEnabled(integration: IntegrationView, enabled: boolean) {
    appsBusy = true;
    appsMessage = "";
    try {
      await invoke("set_integration_enabled", {
        bundleId: integration.bundle_id,
        enabled,
      });
      integration.enabled = enabled;
      appsMessage = `${integration.display_name} ${enabled ? "enabled" : "paused"}.`;
    } catch (error) {
      appsMessage = String(error);
    } finally {
      appsBusy = false;
    }
  }

  async function saveAutoEnableNewApps() {
    appsBusy = true;
    appsMessage = "";
    try {
      autoEnableNewApps = await invoke<boolean>("set_auto_enable_new_apps", {
        enabled: autoEnableNewApps,
      });
      appsMessage = autoEnableNewApps
        ? "New apps will be enabled when first focused."
        : "New apps stay paused until you enable them.";
    } catch (error) {
      appsMessage = String(error);
    } finally {
      appsBusy = false;
    }
  }

  async function refreshAutoAnalyze() {
    try {
      autoAnalyze = await invoke<boolean>("get_auto_analyze");
    } catch (error) {
      autoAnalysisMessage = String(error);
    }
  }

  async function saveAutoAnalyze() {
    autoAnalysisBusy = true;
    autoAnalysisMessage = "";
    try {
      autoAnalyze = await invoke<boolean>("set_auto_analyze", {
        enabled: autoAnalyze,
      });
      autoAnalysisMessage = autoAnalyze
        ? "Elocutio will analyze the focused field after you pause typing."
        : "Automatic analysis is off.";
    } catch (error) {
      autoAnalysisMessage = String(error);
    } finally {
      autoAnalysisBusy = false;
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

  async function refreshServiceStatus() {
    try {
      running = await invoke<boolean>("get_highlighter_service_status");
    } catch (error) {
      message = String(error);
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
      providerMessage =
        providerKind === "open_router"
          ? "OpenRouter accepted the key."
          : "The endpoint accepted the request.";
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

  async function detectTone() {
    detectToneBusy = true;
    try {
      toneResult = await invoke<ToneResponse>("detect_text_tone", { text: analysisText });
    } catch (error) {
      analysisMessage = String(error);
    } finally {
      detectToneBusy = false;
    }
  }

  async function rewrite() {
    providerBusy = true;
    providerMessage = "";
    rewriteOutput = "";
    rewriteMeta = "";
    rewriteDiff = [];
    try {
      const result = await invoke<RewritePreview>("rewrite_preview", {
        text: sourceText,
        model,
      });
      rewriteOutput = result.rewritten;
      rewriteDiff = result.diff;
      const cost = result.cost_usd === null ? "cost unavailable" : `$${result.cost_usd.toFixed(4)}`;
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

  // ---- The rewrite flow -----------------------------------------------------
  //
  // `rewrite_preview` (the button above) returns the diff and the token
  // counts. `rewrite_text` returns the raw provider answer, and
  // `rewrite_text_streamed` asks for the same thing over a streamed request,
  // which is what the app uses for long rewrites. Both are worth being able
  // to see from here: one is what the app does, the other is the fallback
  // path, and a difference between them is a bug worth finding.

  async function rewriteRaw() {
    rawRewriteBusy = true;
    providerMessage = "";
    rawRewrite = null;
    try {
      rawRewrite = await invoke<RewriteResult>("rewrite_text", {
        text: sourceText,
        model,
      });
      void refreshUsage();
    } catch (error) {
      providerMessage = String(error);
    } finally {
      rawRewriteBusy = false;
    }
  }

  async function rewriteStreamed() {
    rawRewriteBusy = true;
    providerMessage = "";
    streamedRewrite = null;
    try {
      streamedRewrite = await invoke<RewriteResult>("rewrite_text_streamed", {
        text: sourceText,
        model,
      });
      void refreshUsage();
    } catch (error) {
      providerMessage = String(error);
    } finally {
      rawRewriteBusy = false;
    }
  }

  /**
   * The prepared system-wide rewrite.
   *
   * The ⌥⌘E shortcut prepares a rewrite of the current selection and leaves it
   * on disk. These three commands are how a user looks at it, applies it, or
   * throws it away.
   */
  async function refreshPendingRewrite() {
    pendingBusy = true;
    pendingMessage = "";
    try {
      pendingRewrite = await invoke<PendingRewrite | null>("get_pending_rewrite");
    } catch (error) {
      pendingMessage = String(error);
    } finally {
      pendingBusy = false;
    }
  }

  async function previewSelectionRewrite() {
    pendingBusy = true;
    pendingMessage = "";
    try {
      pendingRewrite = await invoke<PendingRewrite>("preview_rewrite_selection");
      pendingMessage = "Prepared a rewrite of the current selection. Nothing has changed yet.";
    } catch (error) {
      pendingMessage = String(error);
    } finally {
      pendingBusy = false;
    }
  }

  async function applyPendingRewrite() {
    pendingBusy = true;
    pendingMessage = "";
    try {
      pendingMessage = await invoke<string>("apply_pending_rewrite");
      pendingRewrite = null;
    } catch (error) {
      pendingMessage = String(error);
    } finally {
      pendingBusy = false;
    }
  }

  async function discardPendingRewrite() {
    pendingBusy = true;
    pendingMessage = "";
    try {
      await invoke("discard_pending_rewrite");
      pendingRewrite = null;
      pendingMessage = "The prepared rewrite was discarded.";
    } catch (error) {
      pendingMessage = String(error);
    } finally {
      pendingBusy = false;
    }
  }

  // ---- The resolver quality check -------------------------------------------

  /**
   * Score the quote-to-offset resolver against the seed golden set.
   *
   * This is the number that decides where an edit lands, so it is worth being
   * able to read it from the app rather than only from CI.
   */
  async function runQualityCheck() {
    qualityBusy = true;
    qualityMessage = "";
    resolverQuality = null;
    try {
      resolverQuality = await invoke<ResolverQuality>("run_resolver_quality_check");
      qualityMessage = `Checked ${resolverQuality.quotes} quotes from the seed golden set.`;
    } catch (error) {
      qualityMessage = String(error);
    } finally {
      qualityBusy = false;
    }
  }

  // ---- The app matrix -------------------------------------------------------

  /** Open or close the checklist for one app. */
  function toggleMatrixApp(bundleId: string) {
    matrixOpenApp = matrixOpenApp === bundleId ? null : bundleId;
  }

  /**
   * Record one matrix check.
   *
   * The matrix is a manual test, so the record is what makes it a test rather
   * than a memory. The summary that comes back is the same one the release
   * gate reads, so the gate and the settings window can never disagree.
   */
  async function recordMatrixCheck(bundleId: string, condition: string, state: MatrixState) {
    matrixBusy = true;
    matrixMessage = "";
    try {
      matrixSummary = await invoke<MatrixSummary>("record_app_matrix_check", {
        bundleId,
        condition,
        state,
      });
      matrixChecks = {
        ...matrixChecks,
        [bundleId]: { ...(matrixChecks[bundleId] ?? {}), [condition]: state },
      };
      matrixMessage = "Check recorded.";
    } catch (error) {
      matrixMessage = String(error);
    } finally {
      matrixBusy = false;
    }
  }

  // ---- Diagnostics ----------------------------------------------------------

  async function refreshDefaultModel() {
    try {
      defaultModel = await invoke<string>("get_default_model");
    } catch (error) {
      providerMessage = String(error);
    }
  }

  async function refreshCoalescer() {
    coalescerBusy = true;
    try {
      coalescerState = await invoke<CoalescerState>("get_coalescer_state");
    } catch (error) {
      providerMessage = String(error);
    } finally {
      coalescerBusy = false;
    }
  }

  async function clearCoalescer() {
    coalescerBusy = true;
    try {
      const remaining = await invoke<number>("clear_coalescer_state");
      coalescerState = await invoke<CoalescerState>("get_coalescer_state");
      providerMessage = `Cleared the coalescer. ${remaining} request(s) still tracked.`;
    } catch (error) {
      providerMessage = String(error);
    } finally {
      coalescerBusy = false;
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
    void refreshServiceStatus();
    void refreshProvider();
    void refreshTone();
    void refreshLocalOnly();
    void refreshOnboarding();
    void refreshModel();
    void refreshAutoAnalyze();
    void refreshBudget();
    void refreshIntegrations();
    void refreshVoice();
    void refreshUsage();
    void refreshEvents();
    void loadVoiceTraits();
    void refreshModels();
    void refreshCostCap();
    void refreshInstruction();
    void refreshGate();
    void refreshWidgetAnchors();
    void refreshNetworkLog();
    void refreshMatrix();

    // The commands that had no UI caller until now. Each one is read once on
    // open so the sections are populated, and each has a button that reads it
    // again on demand.
    void refreshVoiceSummary();
    void refreshVoiceTraitDetails();
    void refreshVoiceCounts();
    void refreshDefaultModel();
    void refreshPendingRewrite();
    void refreshCoalescer();

    // Keep the table of contents in step with the section on screen.
    window.addEventListener("scroll", syncActiveSection, { passive: true });
    return () => {
      window.removeEventListener("scroll", syncActiveSection);
    };
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
            <strong>Provider key</strong>
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

  <nav class="toc" aria-label="Settings sections">
    <p class="toc-title">Settings</p>
    <ul class="toc-list">
      {#each sections as section (section.id)}
        <li>
          <a
            href="#{section.id}"
            class:active={activeSection === section.id}
            onclick={(event) => jumpToSection(event, section.id)}
          >
            {section.label}
          </a>
        </li>
      {/each}
    </ul>
  </nav>

  <section class="section" id="general">
    <h2 class="section-title">General</h2>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Suggestions</h3>
          <p class="muted">
            Local correctness checks run on your Mac. Model-backed suggestions are available from
            the floating AI action, or automatically after a pause if you opt in below.
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
        <button
          class="ghost"
          onclick={() => {
            void refresh();
            void refreshServiceStatus();
          }}
          disabled={busy}
        >
          Refresh
        </button>
      </div>

      <label class="switch-row">
        <input
          type="checkbox"
          bind:checked={autoAnalyze}
          onchange={saveAutoAnalyze}
          disabled={autoAnalysisBusy}
        />
        <span>
          <strong>Analyze as I write</strong>
          <span class="muted"
            >Sends the focused field to OpenRouter after a pause. Off by default.</span
          >
        </span>
      </label>

      <label class="switch-row">
        <input
          type="checkbox"
          bind:checked={localOnly}
          onchange={saveLocalOnly}
          disabled={localOnlyBusy}
        />
        <span>
          <strong>Local-only mode</strong>
          <span class="muted">
            Blocks all provider requests; Harper and the local card continue to work.
          </span>
        </span>
      </label>

      {#if localOnlyMessage}
        <p class="message">{localOnlyMessage}</p>
      {/if}

      {#if autoAnalysisMessage}
        <p class="message">{autoAnalysisMessage}</p>
      {/if}

      {#if message}
        <p class="message">{message}</p>
      {/if}
    </section>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Apps</h3>
          <p class="muted">
            Choose where Elocutio may draw suggestions. Paused apps are left untouched.
          </p>
        </div>
        <span class="status on">{integrations.filter((app) => app.enabled).length} active</span>
      </div>

      <ul class="app-list">
        {#each integrations as integration (integration.bundle_id)}
          <li>
            <label class="switch-row">
              <input
                type="checkbox"
                checked={integration.enabled}
                onchange={(event) =>
                  setIntegrationEnabled(
                    integration,
                    (event.currentTarget as HTMLInputElement).checked,
                  )}
                disabled={appsBusy}
              />
              <span>
                <strong>{integration.display_name}</strong>
                <span class="muted">{integration.bundle_id}</span>
              </span>
            </label>
          </li>
        {/each}
      </ul>

      <label class="switch-row">
        <input
          type="checkbox"
          bind:checked={autoEnableNewApps}
          onchange={saveAutoEnableNewApps}
          disabled={appsBusy}
        />
        <span>
          <strong>Enable new apps automatically</strong>
          <span class="muted">Otherwise a newly focused app stays paused until you allow it.</span>
        </span>
      </label>

      <h4 class="card-subtitle">Block list</h4>
      <p class="muted">
        A blocked app is never read, never linted, and never sent to a provider — even if it is also
        enabled above. Blocking an app also turns it off.
      </p>

      <ul class="app-list">
        {#each integrations as integration (integration.bundle_id)}
          <li>
            <label class="switch-row">
              <input
                type="checkbox"
                checked={!blocklist.includes(integration.bundle_id)}
                onchange={(event) =>
                  setBlocked(
                    integration.bundle_id,
                    !(event.currentTarget as HTMLInputElement).checked,
                  )}
                disabled={appsBusy}
              />
              <span>
                <strong>{integration.display_name}</strong>
                <span class="muted">
                  {blocklist.includes(integration.bundle_id) ? "Blocked" : "Allowed"}
                </span>
              </span>
            </label>
          </li>
        {/each}
      </ul>

      {#if appsMessage}
        <p class="message">{appsMessage}</p>
      {/if}

      <h4 class="card-subtitle">Widget position</h4>
      <p class="muted">
        The floating widget sits beside the first underline by default. Pin it to a corner for an
        app to keep it there.
      </p>

      <ul class="app-list">
        {#each integrations as integration (integration.bundle_id)}
          {@const pinned = widgetAnchors.find(([id]) => id === integration.bundle_id)}
          <li>
            <label class="switch-row">
              <select
                value={pinned ? pinned[1].corner : ""}
                onchange={(event) =>
                  setWidgetAnchor(
                    integration.bundle_id,
                    (event.currentTarget as HTMLSelectElement).value || null,
                  )}
                disabled={appsBusy}
              >
                <option value="">Default</option>
                {#each anchorCorners as corner}
                  <option value={corner}>{corner.replace("_", " ")}</option>
                {/each}
              </select>
              <span>
                <strong>{integration.display_name}</strong>
                <span class="muted">
                  {pinned ? "Pinned" : "Default position"}
                </span>
              </span>
            </label>
          </li>
        {/each}
      </ul>
    </section>

    <section class="card">
      <h3 class="card-title">Try the overlay</h3>
      <ol>
        <li>Open TextEdit, Notes, or Safari and focus a text field.</li>
        <li>Type a sentence with a deliberate typo, such as “this is tehre test”.</li>
        <li>
          Elocutio should underline the issue and show the floating suggestion card; use its AI side
          for a model-backed pass.
        </li>
      </ol>

      <div class="actions">
        <button class="ghost" onclick={undoLastEdit} disabled={undoBusy}
          >Undo last Elocutio edit</button
        >
      </div>

      {#if undoMessage}
        <p class="message">{undoMessage}</p>
      {/if}
      <p class="muted">
        If nothing appears, confirm Accessibility is granted, then stop and start suggestions.
        Secure fields, terminals, and Google Docs are intentionally unsupported.
      </p>
    </section>
  </section>

  <section class="section" id="provider">
    <h2 class="section-title">Provider</h2>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Provider</h3>
          <p class="muted">
            Bring your own key or endpoint. Keys are stored in the macOS Keychain, not in config.
          </p>
        </div>
        <span class="status" class:on={keyPresent} class:off={!keyPresent}>
          {keyPresent ? "Key saved" : "No key"}
        </span>
      </div>

      <label class="field">
        <span>Provider</span>
        <select
          value={providerKind}
          onchange={(event) =>
            setProviderKind(
              (event.currentTarget as HTMLSelectElement).value as ProviderSettings["kind"],
            )}
          disabled={providerSettingsBusy}
        >
          <option value="open_router">OpenRouter</option>
          <option value="open_ai_compatible">OpenAI-compatible endpoint</option>
        </select>
      </label>

      {#if providerKind === "open_ai_compatible"}
        <label class="field">
          <span>Base URL</span>
          <div class="model-row">
            <input
              bind:value={providerBaseInput}
              spellcheck="false"
              placeholder="https://api.openai.com/v1"
            />
            <button
              onclick={saveProviderBaseUrl}
              disabled={providerSettingsBusy || !providerBaseInput.trim()}
            >
              Save
            </button>
          </div>
        </label>
        <p class="muted">
          Works with OpenAI, Ollama, LM Studio, and other Chat Completions endpoints. Use a model id
          the endpoint supports.
        </p>
      {/if}

      {#if providerSettingsMessage}
        <p class="message">{providerSettingsMessage}</p>
      {/if}

      {#if !keyPresent}
        <label class="field">
          <span
            >API key {providerKind === "open_ai_compatible"
              ? "(optional for local endpoints)"
              : ""}</span
          >
          <input
            type="password"
            bind:value={keyInput}
            placeholder={providerKind === "open_router" ? "sk-or-v1-…" : "optional"}
            autocomplete="off"
            spellcheck="false"
          />
        </label>
        <div class="actions">
          <button
            class="primary"
            onclick={saveKey}
            disabled={providerBusy || (providerKind === "open_router" && !keyInput.trim())}
          >
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
      <div class="row">
        <div>
          <h3 class="card-title">Model and cost</h3>
          <p class="muted">
            The model every rewrite and analysis uses, and the two ceilings that keep a request from
            costing more than you meant it to.
          </p>
        </div>
        <button class="ghost small" onclick={refreshDefaultModel}>Default model</button>
      </div>

      {#if defaultModel}
        <p class="muted">
          The provider's default model is <code>{defaultModel}</code>. It is used whenever no model
          is chosen explicitly.
        </p>
      {/if}

      <label class="field">
        <span>Model</span>
        {#if modelOptions.length > 0}
          <select bind:value={model} disabled={modelBusy}>
            {#each modelOptions as option (option.model_id)}
              <option value={option.model_id}>
                {option.display_name} — ${option.price_input_per_million_usd}/$${option.price_output_per_million_usd}
                per 1M · {option.best_for}
              </option>
            {/each}
          </select>
          <div class="model-row">
            <input bind:value={model} spellcheck="false" placeholder="Or type a model id" />
            <button onclick={saveModel} disabled={modelBusy || !model.trim()}>Save</button>
          </div>
        {:else}
          <div class="model-row">
            <input bind:value={model} spellcheck="false" />
            <button onclick={saveModel} disabled={modelBusy || !model.trim()}>Save</button>
          </div>
        {/if}
        <p class="muted">
          The list is a local catalog, so it works offline. Prices are approximate and come from the
          provider at the time the catalog was written.
        </p>
      </label>

      {#if modelMessage}
        <p class="message">{modelMessage}</p>
      {/if}

      <label class="field">
        <span>Cost cap (USD per request, optional)</span>
        <div class="model-row">
          <input
            type="number"
            min="0"
            step="0.01"
            bind:value={costCapInput}
            placeholder="e.g. 0.05"
          />
          <button onclick={saveCostCap} disabled={costCapBusy}>Save</button>
        </div>
        <p class="muted">
          A request that would cost more than this is refused before it is sent. This is a hard
          limit; the budget warning below only tells you after the fact.
        </p>
      </label>

      {#if costCapMessage}
        <p class="message">{costCapMessage}</p>
      {/if}

      <h4 class="card-subtitle">Usage</h4>
      <p class="muted">Local metadata only. Text is never written to this log.</p>

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

        {#if budgetUsd !== null && usage.cost_usd > budgetUsd}
          <p class="message warn">
            Recorded spend ${usage.cost_usd.toFixed(4)} is above your ${budgetUsd.toFixed(2)} budget.
          </p>
        {/if}

        <label class="field">
          <span>Budget warning (USD, optional)</span>
          <div class="model-row">
            <input
              type="number"
              min="0"
              step="0.01"
              bind:value={budgetInput}
              placeholder="e.g. 5.00"
            />
            <button onclick={saveBudget} disabled={budgetBusy}>Save</button>
          </div>
        </label>

        {#if budgetMessage}
          <p class="message">{budgetMessage}</p>
        {/if}

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

      <div class="actions">
        <button class="ghost small" onclick={refreshUsage}>Refresh usage</button>
      </div>
    </section>
  </section>

  <section class="section" id="writing">
    <h2 class="section-title">Writing</h2>

    <section class="card">
      <h3 class="card-title">Test a rewrite</h3>
      <p class="muted">
        This proves the selected provider path in-app; the system-wide overlay uses the same
        provider and model.
      </p>

      <label class="field">
        <span>Custom instruction</span>
        <textarea
          bind:value={customInstruction}
          rows="2"
          placeholder="e.g. Prefer British spelling, and keep sentences short."></textarea>
        <div class="model-row">
          <button onclick={saveInstruction} disabled={instructionBusy}>Save</button>
        </div>
        <p class="muted">
          Applied to every rewrite and analysis. The text is sanitized before it reaches a prompt,
          so it cannot be used to fake a prompt boundary.
        </p>
      </label>

      {#if instructionMessage}
        <p class="message">{instructionMessage}</p>
      {/if}

      <label class="field">
        <span>Rewrite tone</span>
        <select bind:value={rewriteTone} onchange={saveTone} disabled={toneBusy}>
          <option value="neutral">Neutral</option>
          <option value="confident">Confident</option>
          <option value="friendly">Friendly</option>
          <option value="concise">Concise</option>
          <option value="formal">Formal</option>
        </select>
      </label>

      {#if toneMessage}
        <p class="message">{toneMessage}</p>
      {/if}

      <label class="field">
        <span>Text</span>
        <textarea bind:value={sourceText} rows="4"></textarea>
      </label>

      <div class="actions">
        <button class="primary" onclick={rewrite} disabled={providerBusy || !keyPresent}>
          Rewrite with provider
        </button>
        <button
          class="ghost"
          onclick={rewriteRaw}
          disabled={rawRewriteBusy || !keyPresent || !sourceText.trim()}
        >
          Rewrite (raw request)
        </button>
        <button
          class="ghost"
          onclick={rewriteStreamed}
          disabled={rawRewriteBusy || !keyPresent || !sourceText.trim()}
        >
          Rewrite (streamed request)
        </button>
      </div>

      <p class="muted">
        All three send the same text to the same model. The first shows a word-level diff, the
        second returns the provider's answer as it arrives, and the third asks for the same thing
        over a streamed request. Run the two raw paths together to check they agree.
      </p>

      {#if rewriteOutput}
        <div class="rewrite">
          <p class="rewrite-label">Result</p>
          {#if rewriteDiff.length > 0}
            <p class="rewrite-text diff-text">
              {#each rewriteDiff as segment}
                <span
                  class:diff-insert={segment.kind === "insert"}
                  class:diff-delete={segment.kind === "delete"}
                  class:diff-equal={segment.kind === "equal"}
                  >{segment.text}
                </span>
              {/each}
            </p>
          {:else}
            <p class="rewrite-text">{rewriteOutput}</p>
          {/if}
          <p class="muted">{rewriteMeta}</p>
        </div>
      {/if}

      {#if rawRewrite}
        <div class="rewrite">
          <p class="rewrite-label">Raw request</p>
          <p class="rewrite-text">{rawRewrite.text}</p>
          <p class="muted">
            {rawRewrite.model} · {rawRewrite.input_tokens} in / {rawRewrite.output_tokens} out ·
            {rawRewrite.cost_usd === null
              ? "cost unavailable"
              : `$${rawRewrite.cost_usd.toFixed(4)}`}
          </p>
        </div>
      {/if}

      {#if streamedRewrite}
        <div class="rewrite">
          <p class="rewrite-label">Streamed request</p>
          <p class="rewrite-text">{streamedRewrite.text}</p>
          <p class="muted">
            {streamedRewrite.model} · {streamedRewrite.input_tokens} in /
            {streamedRewrite.output_tokens} out ·
            {streamedRewrite.cost_usd === null
              ? "cost unavailable"
              : `$${streamedRewrite.cost_usd.toFixed(4)}`}
          </p>
        </div>
      {/if}
    </section>

    <section class="card">
      <h3 class="card-title">Rewrite the selection</h3>
      <p class="muted">
        The <kbd>⌥⌘E</kbd> shortcut prepares a rewrite of the text you have selected in another app and
        leaves it here. Nothing is written to the field until you apply it.
      </p>

      <div class="actions">
        <button class="primary" onclick={previewSelectionRewrite} disabled={pendingBusy}>
          Prepare from the current selection
        </button>
        <button class="ghost" onclick={refreshPendingRewrite} disabled={pendingBusy}>
          Refresh
        </button>
      </div>

      {#if pendingRewrite}
        <div class="rewrite">
          <p class="rewrite-label">Prepared rewrite</p>
          <p class="rewrite-text diff-text">
            {#each pendingRewrite.diff as segment}
              <span
                class:diff-insert={segment.kind === "insert"}
                class:diff-delete={segment.kind === "delete"}
                class:diff-equal={segment.kind === "equal"}
                >{segment.text}
              </span>
            {/each}
          </p>
          <p class="muted">
            {pendingRewrite.model} · prepared {new Date(
              pendingRewrite.created_at * 1000,
            ).toLocaleString()}
          </p>
        </div>

        <div class="actions">
          <button class="primary" onclick={applyPendingRewrite} disabled={pendingBusy}>
            Apply to the selection
          </button>
          <button class="ghost" onclick={discardPendingRewrite} disabled={pendingBusy}>
            Discard
          </button>
        </div>
      {:else}
        <p class="muted">There is no prepared rewrite right now.</p>
      {/if}

      {#if pendingMessage}
        <p class="message">{pendingMessage}</p>
      {/if}
    </section>

    <section class="card">
      <h3 class="card-title">Analyze a document</h3>
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
        <button
          class="ghost"
          onclick={detectTone}
          disabled={detectToneBusy || !analysisText.trim()}
        >
          Detect tone locally
        </button>
      </div>

      {#if analysisMessage}
        <p class="message">{analysisMessage}</p>
      {/if}

      {#if toneResult}
        <div class="tone-result">
          <p>
            <span class="badge">{toneResult.label}</span>
            <span class="muted">
              {(toneResult.confidence * 100).toFixed(0)}% confidence · detected on this Mac, nothing
              was sent
            </span>
          </p>
          {#if toneResult.guidance}
            <p class="muted">{toneResult.guidance}</p>
          {/if}
          <ul class="tone-evidence">
            {#each toneResult.evidence as line}
              <li>{line}</li>
            {/each}
          </ul>
        </div>
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
  </section>

  <section class="section" id="voice">
    <h2 class="section-title">Voice</h2>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Voice</h3>
          <p class="muted">
            Teach Elocutio your rhythm and diction from a sample. Only aggregate traits are stored;
            the sample itself is discarded.
          </p>
        </div>
        <span
          class="status"
          class:on={voiceProfile?.strength !== "off"}
          class:off={!voiceProfile || voiceProfile.strength === "off"}
        >
          {voiceProfile ? (voiceProfile.strength === "off" ? "Off" : "On") : "Not trained"}
        </span>
      </div>

      {#if voiceProfile}
        <p class="voice-summary">{voiceProfile.summary}</p>
        <p class="muted">
          Confidence {Math.round(voiceProfile.confidence * 100)}% · {Object.keys(
            voiceProfile.traits,
          ).length} traits
        </p>
        {#if voiceGuidance}
          <details>
            <summary>What will be sent</summary>
            <pre class="voice-guidance">{voiceGuidance}</pre>
          </details>
        {/if}

        <label class="switch-row">
          <input
            type="checkbox"
            checked={voiceProfile.strength !== "off"}
            onchange={(event) => setVoiceEnabled((event.currentTarget as HTMLInputElement).checked)}
            disabled={voiceBusy}
          />
          <span>
            <strong>Use my voice in rewrites</strong>
            <span class="muted"
              >Adds a short style card to the provider prompt; never sends the sample.</span
            >
          </span>
        </label>
        <div class="voice-rating">
          <span>Does this sound like you?</span>
          <button class="ghost" disabled={voiceRatingBusy} onclick={() => rateVoice("thumbs_up")}>
            Yes
          </button>
          <button class="ghost" disabled={voiceRatingBusy} onclick={() => rateVoice("thumbs_down")}>
            No
          </button>
        </div>
        <p class="muted">
          This rates the last rewrite and folds the answer into your profile straight away.
        </p>

        {#if voiceTraits.length > 0}
          <div class="voice-traits">
            <h4>What Elocutio learned</h4>
            {#each voiceTraits as trait (trait.key)}
              {@const learned = voiceTraitList.find((entry) => entry.key === trait.key)}
              <div class="voice-trait">
                <strong>{trait.key}</strong>
                <span>{trait.value}</span>
                <span class="muted">{Math.round(trait.confidence * 100)}%</span>
                {#if trait.is_override}
                  <span class="tag">set by you</span>
                  <button class="ghost" onclick={() => clearTraitOverride(trait.key)}>Reset</button>
                {/if}
              </div>
              {#if learned && learned.evidence.length > 0}
                <p class="muted voice-evidence">{learned.evidence.join(" · ")}</p>
              {/if}
              <div class="voice-trait-edit">
                <input
                  type="text"
                  value={traitDraft(trait.key, trait.value)}
                  oninput={(event) =>
                    setTraitDraft(trait.key, (event.currentTarget as HTMLInputElement).value)}
                  onkeydown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void saveTrait(trait.key, traitDraft(trait.key, trait.value));
                    }
                  }}
                  spellcheck="false"
                  aria-label={`Value for ${trait.key}`}
                />
                <button
                  class="ghost"
                  disabled={voiceBusy}
                  onclick={() => saveTrait(trait.key, traitDraft(trait.key, trait.value))}
                >
                  Save
                </button>
              </div>
            {/each}
            <p class="muted">
              Type a value and save it to pin that trait. A number, <code>true</code> or
              <code>false</code>, or plain text — the kind follows what you type. Your value always
              wins over what was learned.
            </p>
          </div>
        {/if}

        {#if voiceTraitOverrides.length > 0}
          <h4 class="card-subtitle">Pinned by you</h4>
          <ul class="usage-list">
            {#each voiceTraitOverrides as override (override[0])}
              <li>
                <span>{override[0]}</span>
                <span>{taggedValueText(override[1])}</span>
                <span>{override[1].kind}</span>
              </li>
            {/each}
          </ul>
        {/if}

        <div class="actions">
          <button class="ghost" onclick={clearVoice} disabled={voiceBusy}>Delete profile</button>
        </div>
      {:else}
        <label class="field">
          <span>Writing sample (300+ words)</span>
          <textarea
            bind:value={voiceSamples}
            rows="5"
            placeholder="Paste a few paragraphs of your own writing."></textarea>
        </label>
        <div class="actions">
          <button class="primary" onclick={learnVoice} disabled={voiceBusy || !voiceSamples.trim()}>
            Learn my voice
          </button>
        </div>
      {/if}

      <label class="switch-row">
        <input
          type="checkbox"
          checked={learnFromEdits}
          onchange={(event) => setLearnFromEdits((event.currentTarget as HTMLInputElement).checked)}
          disabled={voiceBusy}
        />
        <span>
          <strong>Learn from my accepted edits</strong>
          <span class="muted">
            Off by default. Accepted edits stay local and update the style card; {voiceFeedbackCount}
            recorded.
          </span>
        </span>
      </label>

      {#if voiceMessage}
        <p class="message">{voiceMessage}</p>
      {/if}
    </section>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Voice diagnostics</h3>
          <p class="muted">
            What the app has stored about your voice, and the guard that stops a rewrite repeating
            your own samples back at you.
          </p>
        </div>
        <button
          class="ghost small"
          onclick={() => {
            void refreshVoiceSummary();
            void refreshVoiceCounts();
          }}
        >
          Refresh
        </button>
      </div>

      {#if voiceSummary}
        <dl class="provider-details">
          <div>
            <dt>Traits</dt>
            <dd>{voiceSummary.trait_count}</dd>
          </div>
          <div>
            <dt>Your overrides</dt>
            <dd>{voiceSummary.override_count}</dd>
          </div>
          <div>
            <dt>Feedback records</dt>
            <dd>{voiceSummary.feedback_count}</dd>
          </div>
          <div>
            <dt>Voice events</dt>
            <dd>{voiceEventCount}</dd>
          </div>
          <div>
            <dt>Edit fingerprints</dt>
            <dd>{feedbackFingerprintCount}</dd>
          </div>
          <div>
            <dt>Sample guards</dt>
            <dd>{voiceSummary.sample_guards}</dd>
          </div>
        </dl>
        <p class="muted">
          {voiceSummary.name} · strength {voiceSummary.strength} · confidence
          {Math.round(voiceSummary.confidence * 100)}% ·
          {voiceSummary.learning_enabled ? "learning is on" : "learning is off"}
        </p>
        <p class="voice-summary">{voiceSummary.summary}</p>
      {:else}
        <p class="muted">No voice profile is stored yet.</p>
      {/if}

      <h4 class="card-subtitle">Copy guard</h4>
      <p class="muted">
        Paste a candidate rewrite and the guard will report which phrases it shares with your
        writing samples. The samples themselves are never shown.
      </p>

      <label class="field">
        <span>Candidate text</span>
        <textarea bind:value={copyGuardText} rows="3" placeholder="Paste a rewrite to check."
        ></textarea>
      </label>

      <div class="actions">
        <button
          class="ghost"
          onclick={checkCopyGuard}
          disabled={copyGuardBusy || !copyGuardText.trim()}
        >
          Check the copy guard
        </button>
      </div>

      {#if copyGuardReport}
        {#if copyGuardReport.clean}
          <p class="message">Nothing in that text repeats your samples.</p>
        {:else}
          <p class="message warn">
            {copyGuardReport.overlaps.length} phrase(s) shared with your samples. A rewrite like this
            would be rejected.
          </p>
          <ul class="usage-list">
            {#each copyGuardReport.overlaps as overlap (overlap.ngram)}
              <li>
                <span>{overlap.ngram}</span>
                <span>words {overlap.candidate_start}–{overlap.candidate_end}</span>
                <span></span>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}

      <h4 class="card-subtitle">Rate something else</h4>
      <p class="muted">
        The buttons above rate the last rewrite. This records a rating for a specific piece of
        writing — a reply, a paragraph — and keeps it with your other local feedback.
      </p>

      <label class="field">
        <span>What are you rating?</span>
        <input
          type="text"
          bind:value={voiceContext}
          placeholder="e.g. the reply to Sam"
          spellcheck="false"
        />
      </label>

      <div class="voice-rating">
        <button
          class="ghost"
          disabled={voiceRatingBusy}
          onclick={() => recordVoiceRating("thumbs_up")}
        >
          Sounds like me
        </button>
        <button
          class="ghost"
          disabled={voiceRatingBusy}
          onclick={() => recordVoiceRating("thumbs_down")}
        >
          Does not sound like me
        </button>
      </div>

      {#if recordedRating}
        <p class="muted">
          Last recorded rating: {recordedRating === "thumbs_up"
            ? "sounds like me"
            : "does not sound like me"}.
        </p>
      {/if}
    </section>
  </section>

  <section class="section" id="privacy">
    <h2 class="section-title">Privacy</h2>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Suggestion events</h3>
          <p class="muted">
            Record which suggestions you accept and dismiss so the voice profile can learn your
            preferences. Only the rule, the category, and the size of the edit are stored — never
            the text.
          </p>
        </div>
        <span class:on={eventsEnabled} class:off={!eventsEnabled} class="status">
          {eventsEnabled ? "Recording" : "Off"}
        </span>
      </div>

      <label class="switch-row">
        <input
          type="checkbox"
          checked={eventsEnabled}
          onchange={(event) => setEventsEnabled((event.currentTarget as HTMLInputElement).checked)}
          disabled={eventsBusy}
        />
        <span>
          <strong>Record suggestion events</strong>
          <span class="muted"
            >Off by default. Stored locally and deleted with the button below.</span
          >
        </span>
      </label>

      {#if eventSummary}
        <dl class="provider-details">
          <div>
            <dt>Accepted</dt>
            <dd>{eventSummary.accepted}</dd>
          </div>
          <div>
            <dt>Dismissed</dt>
            <dd>{eventSummary.dismissed}</dd>
          </div>
          <div>
            <dt>Edited</dt>
            <dd>{eventSummary.edited_after_accept}</dd>
          </div>
        </dl>
      {/if}

      <div class="actions">
        <button class="ghost" onclick={clearEvents} disabled={eventsBusy}
          >Delete recorded events</button
        >
        <button class="ghost" onclick={refreshEvents} disabled={eventsBusy}>Refresh</button>
      </div>

      {#if eventsMessage}
        <p class="message">{eventsMessage}</p>
      {/if}
    </section>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Network log</h3>
          <p class="muted">
            Every host Elocutio has contacted. The log holds the host, the model, the token counts
            and the cost — never the text that was sent.
          </p>
        </div>
        {#if networkSummary}
          <span class="status on">{networkSummary.requests} request(s)</span>
        {/if}
      </div>

      {#if networkSummary && networkSummary.hosts.length > 0}
        <ul class="usage-list">
          {#each networkSummary.hosts as [host, count] (host)}
            <li>
              <span>{host}</span>
              <span>{count} request(s)</span>
            </li>
          {/each}
        </ul>
      {/if}

      {#if networkLog.length > 0}
        <ul class="usage-list">
          {#each networkLog.slice().reverse() as entry (entry.at)}
            <li>
              <span>{new Date(entry.at * 1000).toLocaleString()}</span>
              <span>{entry.host}</span>
              <span>{entry.kind}</span>
              <span class:warn={!entry.ok}>{entry.ok ? "ok" : (entry.error ?? "failed")}</span>
            </li>
          {/each}
        </ul>
      {/if}

      <div class="actions">
        <button class="ghost" onclick={refreshNetworkLog}>Refresh</button>
        <button class="ghost" onclick={clearNetworkLog} disabled={networkLog.length === 0}>
          Delete the log
        </button>
      </div>
    </section>

    <section class="card">
      <h3 class="card-title">What leaves your Mac</h3>
      <p class="muted">
        The key is redacted and the text is shown exactly as it will be sent, so you can check what
        leaves your Mac without reading the source.
      </p>

      <div class="actions">
        <button class="ghost" onclick={previewRequest} disabled={previewBusy}>
          Preview the request
        </button>
      </div>

      {#if requestPreview}
        <div class="tone-result">
          <p>
            <span class="badge">{requestPreview.provider}</span>
            <span class="muted">{requestPreview.host}</span>
          </p>
          <ul class="tone-evidence">
            {#each requestPreview.fields as field (field.name)}
              <li>
                <strong>{field.name}</strong>: {field.value}
              </li>
            {/each}
          </ul>
          <p class="muted">The text that will be sent:</p>
          <pre class="voice-guidance">{requestPreview.text}</pre>
        </div>
      {/if}

      {#if previewMessage}
        <p class="message">{previewMessage}</p>
      {/if}
    </section>
  </section>

  <section class="section" id="quality">
    <h2 class="section-title">Quality</h2>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Resolver quality</h3>
          <p class="muted">
            The quote-to-offset resolver is what decides where an edit lands. This runs the real
            resolver against the seed golden set that ships with the app, so the number can be read
            here rather than only in CI.
          </p>
        </div>
        <button class="ghost small" onclick={runQualityCheck} disabled={qualityBusy}>
          Run the check
        </button>
      </div>

      {#if qualityMessage}
        <p class="message">{qualityMessage}</p>
      {/if}

      {#if resolverQuality}
        <dl class="provider-details">
          <div>
            <dt>Quotes</dt>
            <dd>{resolverQuality.quotes}</dd>
          </div>
          <div>
            <dt>Resolved</dt>
            <dd>{resolverQuality.resolved}</dd>
          </div>
          <div>
            <dt>Refused</dt>
            <dd>{resolverQuality.ambiguous}</dd>
          </div>
        </dl>

        <p class="muted">
          Resolution rate {(resolverQuality.resolution_rate * 100).toFixed(1)}% · ambiguous
          {(resolverQuality.ambiguous_rate * 100).toFixed(1)}% ·
          {resolverQuality.wrong} wrong span(s) · golden set {resolverQuality.version}
        </p>

        {#if resolverQuality.failures.length > 0}
          <details>
            <summary>{resolverQuality.failures.length} failure(s)</summary>
            <ul class="tone-evidence">
              {#each resolverQuality.failures as failure (failure)}
                <li>{failure}</li>
              {/each}
            </ul>
          </details>
        {/if}
      {:else}
        <p class="muted">The check has not been run yet.</p>
      {/if}
    </section>
  </section>

  <section class="section" id="diagnostics">
    <h2 class="section-title">Diagnostics</h2>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Release gate</h3>
          <p class="muted">
            Every check the build plan's M1 gate lists, reported as pass, fail, or needing a human.
            A check that needs a human is not a pass.
          </p>
        </div>
        {#if gateReport}
          <span
            class="status"
            class:on={gateReport.failed === 0 && gateReport.needs_human === 0}
            class:off={gateReport.failed > 0 || gateReport.needs_human > 0}
          >
            {gateReport.failed > 0
              ? `${gateReport.failed} failing`
              : gateReport.needs_human > 0
                ? `${gateReport.needs_human} to check`
                : "Green"}
          </span>
        {/if}
      </div>

      {#if gateReport}
        <ul class="gate-list">
          {#each gateReport.checks as check (check.id)}
            <li class="gate-check {check.state}">
              <span class="gate-state"
                >{check.state === "pass" ? "Pass" : check.state === "fail" ? "Fail" : "Check"}</span
              >
              <div>
                <strong>{check.description}</strong>
                <span class="muted">{check.detail}</span>
              </div>
            </li>
          {/each}
        </ul>
      {/if}

      <div class="actions">
        <button class="ghost" onclick={refreshGate}>Refresh</button>
      </div>
    </section>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Application matrix</h3>
          <p class="muted">
            A manual test, so it cannot be automated. What the app can do is hold the list, hold the
            conditions, and remember what you actually checked — so the matrix is something you run
            rather than something you remember.
          </p>
        </div>
        {#if matrixSummary}
          <span
            class="status"
            class:on={matrixSummary.failed === 0 && matrixSummary.unverified === 0}
            class:off={matrixSummary.failed > 0 || matrixSummary.unverified > 0}
          >
            {matrixSummary.failed > 0
              ? `${matrixSummary.failed} failing`
              : matrixSummary.unverified > 0
                ? `${matrixSummary.unverified} to check`
                : "Green"}
          </span>
        {/if}
      </div>

      {#if matrixSummary}
        <p class="muted">
          {matrixSummary.supported} of {matrixSummary.tier_a} Tier A apps draw in the overlay ·
          {matrixSummary.checked} checks recorded, {matrixSummary.failed} failed,
          {matrixSummary.unverified} still to check, across {matrixConditions.length} conditions.
        </p>
        {#if matrixSummary.failing_apps.length > 0}
          <p class="message warn">
            Failing in {matrixSummary.failing_apps.join(", ")}.
          </p>
        {/if}
      {/if}

      <p class="muted">
        Each mark is written to the record on disk as soon as you set it. Cells you marked in an
        earlier session show as unverified here until you set them again; the totals above always
        come from the saved record.
      </p>

      <ul class="app-list">
        {#each matrixApps as app (app.bundle_id)}
          {@const appChecks = matrixChecks[app.bundle_id] ?? {}}
          <li class="matrix-app">
            <button class="matrix-head" onclick={() => toggleMatrixApp(app.bundle_id)}>
              <span>
                <strong>{app.name}</strong>
                <span class="muted">{app.bundle_id}</span>
              </span>
              <span class="muted">
                {Object.keys(appChecks).length}/{matrixConditions.length} marked here
              </span>
            </button>
            {#if matrixOpenApp === app.bundle_id}
              <ul class="matrix-conditions">
                {#each matrixConditions as condition (condition.name)}
                  <li>
                    <span class="muted">{condition.label}</span>
                    <span class="matrix-states">
                      {#each matrixStates as state (state)}
                        <button
                          class:active={appChecks[condition.name] === state}
                          onclick={() => recordMatrixCheck(app.bundle_id, condition.name, state)}
                          disabled={matrixBusy}
                        >
                          {state}
                        </button>
                      {/each}
                    </span>
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>

      {#if matrixMessage}
        <p class="message">{matrixMessage}</p>
      {/if}

      <div class="actions">
        <button class="ghost" onclick={refreshMatrix}>Refresh</button>
      </div>

      <h4 class="card-subtitle">Tier B</h4>
      <p class="muted">Supported, but not in the first release.</p>
      <ul class="app-list">
        {#each matrixTierB as app (app.bundle_id)}
          <li>
            <span>
              <strong>{app.name}</strong>
              <span class="muted">{app.bundle_id}</span>
            </span>
          </li>
        {/each}
      </ul>

      <h4 class="card-subtitle">Not supported</h4>
      <ul class="app-list">
        {#each matrixUnsupported as app (app.bundleId)}
          <li>
            <span>
              <strong>{app.bundleId}</strong>
              <span class="muted">{app.reason}</span>
            </span>
          </li>
        {/each}
      </ul>
    </section>

    <section class="card">
      <div class="row">
        <div>
          <h3 class="card-title">Request coalescer</h3>
          <p class="muted">
            Two callers asking for the same analysis at the same time share one provider request.
            This shows how many distinct texts are being tracked right now.
          </p>
        </div>
        <button class="ghost small" onclick={refreshCoalescer} disabled={coalescerBusy}>
          Refresh
        </button>
      </div>

      {#if coalescerState}
        <dl class="provider-details">
          <div>
            <dt>Tracked</dt>
            <dd>{coalescerState.tracked}</dd>
          </div>
          <div>
            <dt>Idle</dt>
            <dd>{coalescerState.empty ? "yes" : "no"}</dd>
          </div>
        </dl>
      {:else}
        <p class="muted">The coalescer state has not been read yet.</p>
      {/if}

      <div class="actions">
        <button class="ghost" onclick={clearCoalescer} disabled={coalescerBusy}>
          Forget the tracked requests
        </button>
      </div>
    </section>

    <section class="card">
      <h3 class="card-title">Backend commands this window does not call</h3>
      <p class="muted">
        Every command the app registers is either reachable from this window or listed here with the
        reason it is not. A command that is neither is a feature nobody can reach.
      </p>

      <ul class="skipped-list">
        {#each SKIPPED_COMMANDS as entry (entry.command)}
          <li>
            <code>{entry.command}</code>
            <span class="muted">{entry.reason}</span>
          </li>
        {/each}
      </ul>
    </section>
  </section>

  <footer>
    <span>M0 developer shell</span>
    <a href="https://github.com/pri-te-sh/elocutio" target="_blank" rel="noreferrer">
      Source and roadmap
    </a>
  </footer>
</main>

<style>
  .app-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .app-list .switch-row {
    border-top: none;
    padding: 0.55rem 0;
    margin: 0;
  }

  .switch-row {
    display: flex;
    gap: 0.65rem;
    align-items: flex-start;
    margin-top: 1rem;
    padding-top: 0.9rem;
    border-top: 1px solid var(--line);
    font-size: 0.9rem;
  }

  .switch-row input[type="checkbox"] {
    width: auto;
    flex: 0 0 auto;
    margin-top: 0.2rem;
    accent-color: var(--accent);
  }

  .switch-row span {
    min-width: 0;
  }

  .switch-row strong,
  .switch-row .muted {
    display: block;
  }
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

  /* The section headings, one per group of cards. */
  .section {
    /* Clears the sticky table of contents when a nav link is followed. */
    scroll-margin-top: 72px;
    margin-bottom: 8px;
  }

  .section-title {
    margin: 30px 0 12px;
    color: var(--accent);
    font-size: 13px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.09em;
  }

  .section:first-of-type .section-title {
    margin-top: 0;
  }

  .card-title {
    margin: 0 0 6px;
    color: var(--accent);
    font-size: 17px;
    font-weight: 600;
  }

  .card-subtitle {
    margin: 22px 0 6px;
    color: var(--near-black);
    font-size: 13px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
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

  .tone-result {
    margin-top: 12px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-2, transparent);
  }

  .tone-result p {
    margin: 0 0 6px;
    display: flex;
    gap: 8px;
    align-items: baseline;
    flex-wrap: wrap;
  }

  .tone-evidence {
    margin: 6px 0 0;
    padding-left: 18px;
    color: var(--muted);
    font-size: 12px;
  }

  .tone-evidence li {
    margin: 2px 0;
  }

  .voice-rating {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 10px 0;
    flex-wrap: wrap;
  }

  .voice-rating span {
    color: var(--muted);
    font-size: 12px;
  }

  .voice-traits {
    margin: 12px 0;
  }

  .voice-traits h4 {
    margin: 0 0 6px;
    font-size: 12px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .voice-trait {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    flex-wrap: wrap;
  }

  .voice-trait strong {
    font-weight: 600;
  }

  .tag {
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 6px;
    font-size: 10.5px;
    color: var(--muted);
  }

  .gate-list {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .gate-check {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }

  .gate-check div {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .gate-state {
    flex: 0 0 auto;
    min-width: 44px;
    text-align: center;
    border-radius: 999px;
    padding: 0 7px;
    border: 1px solid var(--border);
    font-size: 10.5px;
    color: var(--muted);
  }

  .gate-check.pass .gate-state {
    border-color: var(--ok, var(--border));
    color: var(--ok, var(--muted));
  }

  .gate-check.fail .gate-state {
    border-color: var(--bad);
    color: var(--bad);
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

  .voice-guidance {
    white-space: pre-wrap;
    background: var(--surface-2, rgba(127, 127, 127, 0.08));
    border-radius: 8px;
    padding: 0.7rem;
    font: inherit;
    font-size: 0.82rem;
  }

  .diff-insert {
    background: rgba(14, 173, 105, 0.16);
    color: #0b6b43;
    border-radius: 3px;
  }

  .diff-delete {
    background: rgba(238, 66, 102, 0.14);
    color: #a31336;
    text-decoration: line-through;
    border-radius: 3px;
  }

  .diff-equal {
    color: inherit;
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

  /* ---- The table of contents ---- */

  .toc {
    position: sticky;
    top: 0;
    z-index: 10;
    margin: 0 0 22px;
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--white);
  }

  .toc-title {
    margin: 0 0 8px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.09em;
  }

  .toc-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .toc-list a {
    display: inline-block;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 4px 11px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    text-decoration: none;
  }

  .toc-list a:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  .toc-list a.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
  }

  /* ---- Editing a trait by hand ---- */

  .voice-trait-edit {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 4px 0 10px;
  }

  .voice-trait-edit input {
    flex: 1;
    font-size: 12px;
    padding: 6px 9px;
  }

  /* ---- The app matrix checklist ---- */

  .matrix-app {
    border-top: 1px solid var(--border);
    padding-top: 8px;
  }

  .matrix-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    border: none;
    padding: 6px 0;
    background: transparent;
    text-align: left;
  }

  .matrix-head:hover strong {
    color: var(--accent);
  }

  .matrix-head span span {
    display: block;
  }

  .matrix-conditions {
    list-style: none;
    margin: 0 0 10px;
    padding: 8px 0 0;
    border-top: 1px dashed var(--border);
    display: grid;
    gap: 6px;
  }

  .matrix-conditions li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: 12px;
  }

  .matrix-states {
    display: flex;
    gap: 4px;
    flex: none;
  }

  .matrix-states button {
    padding: 3px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    color: var(--muted);
  }

  .matrix-states button.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
  }

  code {
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 0 4px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.9em;
  }

  .voice-evidence {
    margin: 2px 0 0;
    font-size: 11.5px;
  }

  .skipped-list {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: grid;
    gap: 10px;
  }

  .skipped-list li {
    display: flex;
    flex-direction: column;
    gap: 3px;
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }

  .skipped-list li:first-child {
    border-top: 0;
    padding-top: 0;
  }

  .skipped-list code {
    align-self: flex-start;
    font-size: 12px;
  }
</style>
