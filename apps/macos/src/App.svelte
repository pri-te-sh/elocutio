<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  // ---------------------------------------------------------------------------
  // Shared Types
  // ---------------------------------------------------------------------------
  interface EloRect {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  interface EloSuggestion {
    kind: string;
    label: string;
    replacement: string;
  }

  interface EloAlert {
    id: string;
    rule_name: string;
    category: string;
    category_label: string;
    subcategory: string;
    color: string;
    original: string;
    message: string;
    context_before: string;
    context_after: string;
    suggestions: EloSuggestion[];
    char_start: number;
    char_end: number;
    rect: EloRect;
  }

  interface EloOverlaySnapshot {
    service_enabled: boolean;
    ax_trusted: boolean;
    field_active: boolean;
    pid: number;
    app_bundle_id: string;
    app_name: string;
    field_rect: EloRect | null;
    widget_rect: EloRect | null;
    alerts: EloAlert[];
    tone: string | null;
    score: number;
    word_count: number;
    source_text: string;
    card_command: string | null;
  }

  interface EloRewriteResponse {
    original: string;
    rewritten: string;
    mode_label: string;
    engine: string;
  }

  interface LocalLintSuggestion {
    kind: string;
    text: string;
    label: string;
  }

  interface LocalLintItem {
    id: string;
    rule_name: string;
    kind: string;
    category: string;
    category_label: string;
    subcategory: string;
    color: string;
    start: number;
    end: number;
    original: string;
    message: string;
    context_before: string;
    context_after: string;
    suggestions: LocalLintSuggestion[];
  }

  interface IntegrationItem {
    bundle_id: string;
    enabled: boolean;
    display_name: string;
  }

  interface LintRuleView {
    key: string;
    enabled: boolean;
  }

  interface ProviderSettings {
    kind: string;
    base_url: string;
    model: string;
    key_present: boolean;
    key_hint?: string | null;
  }

  interface PresetModelSlug {
    slug: string;
    label: string;
    badge: string;
    is_live: boolean;
  }

  interface UsageSummaryView {
    requests: number;
    input_tokens: number;
    output_tokens: number;
    cost_usd: number;
  }

  interface ProviderPresetDef {
    id: string;
    name: string;
    tag: string;
    kind: "open_router" | "open_ai_compatible";
    baseUrl: string;
    defaultModel: string;
    requiresKey: boolean;
  }

  const PROVIDER_PRESETS: ProviderPresetDef[] = [
    {
      id: "openrouter",
      name: "OpenRouter",
      tag: "Cloud · 300+ Models",
      kind: "open_router",
      baseUrl: "https://openrouter.ai/api/v1",
      defaultModel: "anthropic/claude-haiku-4.5",
      requiresKey: true,
    },
    {
      id: "ollama_local",
      name: "Ollama (Local)",
      tag: "On-Device · Localhost",
      kind: "open_ai_compatible",
      baseUrl: "http://localhost:11434/v1",
      defaultModel: "llama3.2:3b",
      requiresKey: false,
    },
    {
      id: "ollama_cloud",
      name: "Ollama Cloud",
      tag: "Cloud · Hosted",
      kind: "open_ai_compatible",
      baseUrl: "https://ollama.com/v1",
      defaultModel: "llama3.3:70b",
      requiresKey: true,
    },
    {
      id: "apple_foundation",
      name: "Apple Foundation",
      tag: "macOS · On-Device",
      kind: "open_ai_compatible",
      baseUrl: "http://127.0.0.1:11535/v1",
      defaultModel: "apple/foundation-on-device",
      requiresKey: false,
    },
    {
      id: "openai",
      name: "OpenAI",
      tag: "Cloud · Direct",
      kind: "open_ai_compatible",
      baseUrl: "https://api.openai.com/v1",
      defaultModel: "gpt-4.1-mini",
      requiresKey: true,
    },
    {
      id: "groq",
      name: "Groq Cloud",
      tag: "Cloud · Fast LPU",
      kind: "open_ai_compatible",
      baseUrl: "https://api.groq.com/openai/v1",
      defaultModel: "llama-3.3-70b-versatile",
      requiresKey: true,
    },
  ];

  interface VoiceTraitView {
    key: string;
    value?: string | number | boolean;
    label?: string;
    summary?: string;
    evidence?: string[];
    confidence: number;
    is_override?: boolean;
    overridden?: boolean;
  }

  // ---------------------------------------------------------------------------
  // Mode Detection: `#elo-overlay` vs Desktop Studio & Settings
  // ---------------------------------------------------------------------------
  let isOverlayWindow = $state(
    typeof window !== "undefined" && window.location.hash === "#elo-overlay",
  );

  // ---------------------------------------------------------------------------
  // System-Wide "elo" Overlay State
  // ---------------------------------------------------------------------------
  let overlaySnap = $state<EloOverlaySnapshot>({
    service_enabled: true,
    ax_trusted: true,
    field_active: false,
    pid: 0,
    app_bundle_id: "",
    app_name: "",
    field_rect: null,
    widget_rect: null,
    alerts: [],
    tone: null,
    score: 100,
    word_count: 0,
    source_text: "",
    card_command: null,
  });

  // Overlay popover modes:
  // - "closed": only underlines + resting/hover-expanded "elo" pill
  // - "mini": compact inline quick-fix card under hovered/clicked word (Image 3)
  // - "rewrite": "What do you want to do?" generative rewrite card (Image 5)
  // - "full": full multi-alert suggestion card with context sentence & 5 categories
  type OverlayPopoverMode = "closed" | "mini" | "rewrite" | "full";
  let overlayPopoverMode = $state<OverlayPopoverMode>("closed");
  let overlayActiveAlertId = $state<string | null>(null);
  let overlayHoveredAlertId = $state<string | null>(null);
  let overlayCategoryFilter = $state<string>("all");
  let overlayMoreMenuOpen = $state(false);

  // Hover-expanded "elo" pill toolbar state (Images 1, 2, 4)
  let eloPillHovered = $state(false);
  let eloPillHoveredBtn = $state<"power" | "tone" | "rewrite" | "count" | null>(null);
  let eloPowerMenuOpen = $state(false);

  // "What do you want to do?" Rewrite card state (Image 5)
  let rewriteMoreIdeasOpen = $state(false);
  let rewriteCustomPrompt = $state("");
  let rewriteLoading = $state(false);
  let rewriteError = $state<string | null>(null);
  let rewritePreview = $state<EloRewriteResponse | null>(null);
  let rewriteHistory = $state<EloRewriteResponse[]>([]);

  // Dragging state for system-wide "elo" widget
  let isDraggingElo = $state(false);
  let dragStartPointer = $state({ x: 0, y: 0 });
  let dragStartWidget = $state({ x: 0, y: 0 });
  let dragCurrentOffset = $state({ x: 0, y: 0 });
  let draggedDuringPress = $state(false);

  // ---------------------------------------------------------------------------
  // Desktop Studio & Settings State
  // ---------------------------------------------------------------------------
  type StudioTab = "editor" | "analytics" | "apps" | "rules" | "voice" | "ai";
  let activeTab = $state<StudioTab>("editor");

  const SAMPLE_WELCOME = `Welcome to the Elocutio Editor

Wait—what is Elocutio? Elocutio is a private writing companion that refines clarity, correctness, and tone while preserving your voice.

 Their are several common mistakes in this short passage so you can test inline underlines and the "elo" widget right away. Each suggestion explains why the edit helps, and you can accept or dismiss it with one click.

In the middle column, you can see all alerts grouped in one feed. Click any underlined word on the canvas, or hover the floating "elo" badge in the bottom-right corner of the page, to step through corrections or open "Rewrite with elo".

 Because Elocutio runs a fast local engine right on your Mac, spelling, grammar, punctuation, and conciseness checks happen instantaneously without sending your private drafts to the cloud.`;

  const SAMPLE_BUDGET = `Budget-friendly transformations

For a fast, inexpensive kitchen transformation, consider painting the cabinets. New hardware can also make a big difference. Wait until you see how much brighter the room feels.

 You can also replace light fixtures and add a peel-and-stick backsplash. These simple upgrades are very unique and help create a welcoming space without a major renovation budget.`;

  let docTitle = $state("Welcome to the Elocutio Editor");
  let editorText = $state(SAMPLE_WELCOME);
  let editorLints = $state<LocalLintItem[]>([]);
  let editorSelectedLintId = $state<string | null>(null);
  let editorCategoryFilter = $state<string>("all");
  let editorTone = $state<string>("Neutral");
  let editorInlineCardPos = $state<{ x: number; y: number } | null>(null);
  let editorInlineCardMode = $state<"mini" | "rewrite">("mini");
  let backdropEl = $state<HTMLDivElement | null>(null);
  let textareaEl = $state<HTMLTextAreaElement | null>(null);

  // Draggable & hover-expandable inline "elo" widget inside the Studio Editor
  let studioEloOffset = $state({ x: 0, y: 0 });
  let studioEloDragging = $state(false);
  let studioEloDragStart = $state({ px: 0, py: 0, ox: 0, oy: 0 });
  let studioEloMoved = $state(false);
  let studioEloHovered = $state(false);
  let studioEloHoveredBtn = $state<"tone" | "rewrite" | "count" | null>(null);
  let studioGoalsOpen = $state(false);
  let studioCustomPrompt = $state("");

  // AI Rewrite state inside Editor
  let aiRewriteLoading = $state(false);
  let aiRewriteError = $state<string | null>(null);
  let aiRewriteResult = $state<string | null>(null);
  let selectedRewriteTone = $state<string>("improve");

  // Settings & Configuration State
  let axStatus = $state<string>("Granted");
  let serviceRunning = $state<boolean>(true);
  let autoEnableNewApps = $state<boolean>(true);
  let integrations = $state<IntegrationItem[]>([]);
  let appSearchQuery = $state<string>("");

  let dialectName = $state<string>("US");
  let dictionaryWords = $state<string[]>([]);
  let newDictWord = $state<string>("");
  let lintRules = $state<LintRuleView[]>([]);
  let ruleSearchQuery = $state<string>("");
  let ruleFilterMode = $state<"all" | "enabled" | "disabled">("all");

  interface LearnedEditSummary {
    kind: string;
    category: string;
    original: string;
    replacement: string;
    timestamp: number;
  }

  interface PersonalizationDbStatus {
    db_path: string;
    db_filename: string;
    db_size_bytes: number;
    schema_version: number;
    dictionary_word_count: number;
    builtin_word_count: number;
    customized_rule_count: number;
    total_rule_count: number;
    learned_events_count: number;
    voice_traits_count: number;
    learn_from_edits: boolean;
    recent_learned_edits: LearnedEditSummary[];
  }

  let personalizationDb = $state<PersonalizationDbStatus | null>(null);

  interface CategoryBreakdownItem {
    category: string;
    accepted: number;
    dismissed: number;
    mastery_pct: number;
    insight: string;
  }

  interface ModelUsageBreakdown {
    model: string;
    requests: number;
    input_tokens: number;
    output_tokens: number;
    words_refined: number;
    cost_usd: number;
  }

  interface DailyActivityPoint {
    day_label: string;
    ai_passes: number;
    inline_edits: number;
  }

  interface WritingAnalyticsSnapshot {
    prose_craft_score: number;
    acceptance_rate_pct: number;
    total_accepted_edits: number;
    total_dismissed_edits: number;
    total_ai_requests: number;
    total_input_tokens: number;
    total_output_tokens: number;
    total_ai_words_refined: number;
    total_cost_usd: number;
    avg_cost_per_pass_usd: number;
    estimated_minutes_saved: number;
    vocabulary_shield_words: number;
    active_rules_count: number;
    voice_traits_calibrated: number;
    active_days_14d: number;
    local_vs_ai_pct: number;
    category_breakdown: CategoryBreakdownItem[];
    model_breakdown: ModelUsageBreakdown[];
    daily_activity: DailyActivityPoint[];
    top_transformations: LearnedEditSummary[];
  }

  let writingAnalytics = $state<WritingAnalyticsSnapshot | null>(null);

  let voiceSampleText = $state<string>("");
  let voiceTraits = $state<VoiceTraitView[]>([]);
  let learnFromEdits = $state<boolean>(false);
  let voiceStatusMsg = $state<string | null>(null);

  let localOnlyMode = $state<boolean>(false);
  let providerSettings = $state<ProviderSettings>({
    kind: "open_router",
    base_url: "https://openrouter.ai/api/v1",
    model: "anthropic/claude-haiku-4.5",
    key_present: false,
    key_hint: null,
  });
  let selectedPresetId = $state<string>("openrouter");
  let presetModelSlugs = $state<PresetModelSlug[]>([]);
  let fetchingModels = $state<boolean>(false);
  let customModelSlugMode = $state<boolean>(false);
  let usageSummary = $state<UsageSummaryView>({
    requests: 0,
    input_tokens: 0,
    output_tokens: 0,
    cost_usd: 0,
  });
  let budgetInput = $state<string>("");
  let costCapInput = $state<string>("");
  let apiKeyInput = $state<string>("");
  let customInstruction = $state<string>("");
  let providerStatusMsg = $state<string | null>(null);
  let providerStatusIsError = $state<boolean>(false);

  // ---------------------------------------------------------------------------
  // Helper: Short Action Label for Compact Mini-Popover (Image 3)
  // ---------------------------------------------------------------------------
  function shortActionHeadline(subcategory: string, message: string, sugKind?: string): string {
    const sub = subcategory.toLowerCase();
    const msg = message.toLowerCase();
    if (sub.includes("capital") || msg.includes("capital")) {
      return "Capitalize the word";
    }
    if (sub.includes("spell") || msg.includes("spell") || msg.includes("typo")) {
      return "Correct your spelling";
    }
    if (sub.includes("punct") || msg.includes("comma") || msg.includes("hyphen")) {
      return "Fix punctuation";
    }
    if (sugKind === "remove" || sub.includes("concise") || msg.includes("redundant")) {
      return "Remove redundant word";
    }
    if (sub.includes("vocab") || sub.includes("word")) {
      return "Enhance word choice";
    }
    if (sub.includes("grammar") || sub.includes("agreement")) {
      return "Correct grammar";
    }
    return message.length <= 34 ? message : `Update ${subcategory.toLowerCase()}`;
  }

  // ---------------------------------------------------------------------------
  // Derived Helpers for Overlay Mode
  // ---------------------------------------------------------------------------
  let filteredOverlayAlerts = $derived(
    overlayCategoryFilter === "all"
      ? overlaySnap.alerts
      : overlaySnap.alerts.filter((a) => a.category === overlayCategoryFilter),
  );

  let activeOverlayAlert = $derived.by(() => {
    const list = filteredOverlayAlerts.length > 0 ? filteredOverlayAlerts : overlaySnap.alerts;
    if (list.length === 0) return null;
    if (overlayActiveAlertId) {
      const found = list.find((a) => a.id === overlayActiveAlertId);
      if (found) return found;
    }
    return list[0];
  });

  let activeOverlayAlertIndex = $derived.by(() => {
    if (!activeOverlayAlert) return 0;
    const idx = overlaySnap.alerts.findIndex((a) => a.id === activeOverlayAlert.id);
    return idx >= 0 ? idx : 0;
  });

  // Base resting widget rect (right-edge anchored)
  let baseWidgetRect = $derived.by((): EloRect | null => {
    if (!overlaySnap.widget_rect || !overlaySnap.field_rect) return null;
    if (isDraggingElo) {
      const fr = overlaySnap.field_rect;
      const w = overlaySnap.widget_rect.width;
      const h = overlaySnap.widget_rect.height;
      const minX = fr.x + 6;
      const maxX = Math.max(minX, fr.x + fr.width - w - 8);
      const minY = fr.height < 56 ? Math.max(32, fr.y - h - 4) : fr.y + 6;
      const maxY =
        fr.height < 56 ? fr.y + fr.height + 12 : Math.max(minY, fr.y + fr.height - h - 8);
      return {
        x: Math.max(minX, Math.min(maxX, dragStartWidget.x + dragCurrentOffset.x)),
        y: Math.max(minY, Math.min(maxY, dragStartWidget.y + dragCurrentOffset.y)),
        width: w,
        height: h,
      };
    }
    return overlaySnap.widget_rect;
  });

  // Interactive hit rect reported to Rust (includes leftward hover expansion + top tooltip)
  let effectiveWidgetRect = $derived.by((): EloRect | null => {
    if (!baseWidgetRect) return null;
    if (eloPillHovered || eloPowerMenuOpen) {
      const extraLeft = 74;
      const extraTop = eloPowerMenuOpen ? 120 : 44;
      return {
        x: Math.max(4, baseWidgetRect.x - extraLeft),
        y: Math.max(24, baseWidgetRect.y - extraTop),
        width: baseWidgetRect.width + extraLeft + 8,
        height: baseWidgetRect.height + extraTop + 6,
      };
    }
    return baseWidgetRect;
  });

  let effectiveCardRect = $derived.by((): EloRect | null => {
    if (overlayPopoverMode === "closed" || !overlaySnap.field_active) {
      return null;
    }

    const rawW = typeof window !== "undefined" ? window.innerWidth : 1440;
    const rawH = typeof window !== "undefined" ? window.innerHeight : 900;
    const winW = Math.max(
      rawW,
      overlaySnap.field_rect ? overlaySnap.field_rect.x + overlaySnap.field_rect.width + 24 : 0,
    );
    const winH = Math.max(
      rawH,
      overlaySnap.field_rect ? overlaySnap.field_rect.y + overlaySnap.field_rect.height + 24 : 0,
    );

    if (overlayPopoverMode === "mini" && activeOverlayAlert) {
      const cardW = 246;
      const cardH = 168;
      const anchor = activeOverlayAlert.rect;
      const x = Math.max(12, Math.min(anchor.x, winW - cardW - 12));
      let y = anchor.y + anchor.height + 8;
      if (y + cardH > winH - 16) {
        y = Math.max(32, anchor.y - cardH - 8);
      }
      return { x, y, width: cardW, height: cardH };
    }

    if (overlayPopoverMode === "rewrite") {
      const cardW = 368;
      const cardH = rewriteLoading ? 162 : rewritePreview ? 345 : rewriteMoreIdeasOpen ? 412 : 308;
      const anchor = baseWidgetRect ?? overlaySnap.field_rect;
      if (!anchor) return null;
      const x = Math.max(16, Math.min(anchor.x + anchor.width - cardW, winW - cardW - 16));
      let y = anchor.y - cardH - 12;
      if (y < 36) {
        y = Math.min(winH - cardH - 16, anchor.y + anchor.height + 10);
      }
      return { x, y, width: cardW, height: cardH };
    }

    // "full" multi-alert card
    const cardW = 410;
    const cardH = overlaySnap.alerts.length > 1 ? 370 : 250;
    const anchor = activeOverlayAlert?.rect ?? baseWidgetRect;
    if (!anchor) return null;

    let x = anchor.x;
    if (anchor === baseWidgetRect) {
      x = anchor.x + anchor.width - cardW;
    }
    x = Math.max(16, Math.min(x, winW - cardW - 16));

    let y = anchor.y + anchor.height + 10;
    if (y + cardH > winH - 20) {
      y = Math.max(36, anchor.y - cardH - 12);
    }
    return { x, y, width: cardW, height: cardH };
  });

  // ---------------------------------------------------------------------------
  // Unified Category-Weighted, Document-Length-Aware Writing Score Model
  // Shared between the system-wide "elo" widget and Elocutio Studio Editor.
  // ---------------------------------------------------------------------------
  const CATEGORY_WEIGHTS: Record<string, number> = {
    correctness: 1.35,
    clarity: 1.0,
    engagement: 0.75,
    delivery: 0.75,
    style: 0.85,
  };

  function computeCategoryScore(
    wordCount: number,
    issueCount: number,
    categoryWeight: number,
  ): number {
    if (wordCount === 0 || issueCount === 0) return 100;
    const effectiveWords = Math.max(25, wordCount);
    const ratePer100 = (issueCount / effectiveWords) * 100;
    const penalty = Math.round(
      ratePer100 * 6.5 * categoryWeight + issueCount * 3.5 * categoryWeight,
    );
    return Math.max(15, Math.min(100, 100 - penalty));
  }

  function computeCategoryScoresMap(
    wordCount: number,
    counts: Record<string, number>,
  ): Record<string, number> {
    return {
      correctness: computeCategoryScore(
        wordCount,
        counts.correctness ?? 0,
        CATEGORY_WEIGHTS.correctness,
      ),
      clarity: computeCategoryScore(wordCount, counts.clarity ?? 0, CATEGORY_WEIGHTS.clarity),
      engagement: computeCategoryScore(
        wordCount,
        counts.engagement ?? 0,
        CATEGORY_WEIGHTS.engagement,
      ),
      delivery: computeCategoryScore(wordCount, counts.delivery ?? 0, CATEGORY_WEIGHTS.delivery),
      style: computeCategoryScore(wordCount, counts.style ?? 0, CATEGORY_WEIGHTS.style),
    };
  }

  function computeOverallWritingScore(
    wordCount: number,
    counts: Record<string, number>,
    catScores: Record<string, number>,
  ): number {
    const totalIssues =
      (counts.correctness ?? 0) +
      (counts.clarity ?? 0) +
      (counts.engagement ?? 0) +
      (counts.delivery ?? 0) +
      (counts.style ?? 0);
    if (wordCount === 0 || totalIssues === 0) return 100;

    const weightedSum =
      (catScores.correctness ?? 100) * 0.4 +
      (catScores.clarity ?? 100) * 0.25 +
      (catScores.engagement ?? 100) * 0.12 +
      (catScores.delivery ?? 100) * 0.13 +
      (catScores.style ?? 100) * 0.1;

    const effectiveWords = Math.max(25, wordCount);
    const totalRate = (totalIssues / effectiveWords) * 100;
    const volumePenalty = Math.min(18, Math.round(totalRate * 0.9));

    return Math.max(10, Math.min(99, Math.round(weightedSum) - volumePenalty));
  }

  function scoreColorHex(score: number): string {
    if (score >= 90) return "#10B981";
    if (score >= 75) return "#191F70";
    if (score >= 60) return "#D97706";
    return "#E5484D";
  }

  let overlayCategoryCounts = $derived.by(() => {
    const counts: Record<string, number> = {
      correctness: 0,
      clarity: 0,
      engagement: 0,
      delivery: 0,
      style: 0,
    };
    for (const a of overlaySnap.alerts) {
      counts[a.category] = (counts[a.category] ?? 0) + 1;
    }
    return counts;
  });

  let overlayCategoryScores = $derived.by(() =>
    computeCategoryScoresMap(overlaySnap.word_count, overlayCategoryCounts),
  );

  let overlayScore = $derived.by(() =>
    computeOverallWritingScore(
      overlaySnap.word_count,
      overlayCategoryCounts,
      overlayCategoryScores,
    ),
  );

  let hasStyleOrClarityAlerts = $derived(
    (overlayCategoryCounts.clarity ?? 0) +
      (overlayCategoryCounts.engagement ?? 0) +
      (overlayCategoryCounts.delivery ?? 0) +
      (overlayCategoryCounts.style ?? 0) >
      0,
  );

  // ---------------------------------------------------------------------------
  // Derived Helpers for Studio Editor Mode
  // ---------------------------------------------------------------------------
  let filteredEditorLints = $derived(
    editorCategoryFilter === "all"
      ? editorLints
      : editorLints.filter((l) => l.category === editorCategoryFilter),
  );

  let activeEditorLint = $derived.by(() => {
    if (filteredEditorLints.length === 0) return null;
    if (editorSelectedLintId) {
      const found = filteredEditorLints.find((l) => l.id === editorSelectedLintId);
      if (found) return found;
    }
    return filteredEditorLints[0];
  });

  let editorCategoryCounts = $derived.by(() => {
    const counts: Record<string, number> = {
      correctness: 0,
      clarity: 0,
      engagement: 0,
      delivery: 0,
      style: 0,
    };
    for (const l of editorLints) {
      counts[l.category] = (counts[l.category] ?? 0) + 1;
    }
    return counts;
  });

  let editorWordCount = $derived.by(() => {
    const trimmed = editorText.trim();
    if (!trimmed) return 0;
    return trimmed.split(/\s+/).length;
  });

  let editorCharCount = $derived(editorText.length);

  let editorCategoryScores = $derived.by(() =>
    computeCategoryScoresMap(editorWordCount, editorCategoryCounts),
  );

  let editorScore = $derived.by(() =>
    computeOverallWritingScore(editorWordCount, editorCategoryCounts, editorCategoryScores),
  );

  let editorLexicalDiversity = $derived.by(() => {
    const words = editorText.toLowerCase().match(/[a-z0-9']+/g);
    if (!words || words.length === 0) return 0;
    const unique = new Set(words).size;
    return Math.round((unique / words.length) * 100);
  });

  let editorAvgSentenceWords = $derived.by(() => {
    const trimmed = editorText.trim();
    if (!trimmed) return 0;
    const sentences = trimmed
      .split(/[.!?]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (sentences.length === 0) return editorWordCount;
    return Math.round(editorWordCount / sentences.length);
  });

  let editorReadingTimeLabel = $derived.by(() => {
    if (editorWordCount === 0) return "0s";
    const seconds = Math.max(5, Math.round((editorWordCount / 225) * 60));
    if (seconds < 60) return `${seconds}s`;
    return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  });

  let maxDailyActivity = $derived.by(() => {
    if (!writingAnalytics || writingAnalytics.daily_activity.length === 0) {
      return 1;
    }
    return Math.max(1, ...writingAnalytics.daily_activity.map((d) => d.ai_passes + d.inline_edits));
  });

  interface TextSegment {
    text: string;
    lint: LocalLintItem | null;
  }

  let highlightedSegments = $derived.by((): TextSegment[] => {
    const chars = Array.from(editorText);
    if (editorLints.length === 0) {
      return [{ text: editorText, lint: null }];
    }
    const segs: TextSegment[] = [];
    let cursor = 0;
    for (const lint of editorLints) {
      const start = Math.max(cursor, Math.min(lint.start, chars.length));
      const end = Math.max(start, Math.min(lint.end, chars.length));
      if (start > cursor) {
        segs.push({
          text: chars.slice(cursor, start).join(""),
          lint: null,
        });
      }
      if (end > start) {
        segs.push({
          text: chars.slice(start, end).join(""),
          lint,
        });
      }
      cursor = end;
    }
    if (cursor < chars.length) {
      segs.push({
        text: chars.slice(cursor).join(""),
        lint: null,
      });
    }
    return segs;
  });

  const SYSTEM_DAEMON_PATTERNS = [
    ".xpc.",
    "uiagent",
    "universalaccessauthwarn",
    "loginwindow",
    "notificationcenter",
    "spotlight",
    "systempreferences",
    "systemsettings",
    "controlcenter",
    "windowmanager",
    "coreservices",
    "dock",
  ];

  function isSystemBackgroundHelper(item: IntegrationItem): boolean {
    const id = item.bundle_id.toLowerCase();
    const name = item.display_name.toLowerCase();
    if (name.startsWith("com.apple.")) return true;
    return SYSTEM_DAEMON_PATTERNS.some((pat) => id.includes(pat) || name === pat);
  }

  let filteredIntegrations = $derived(
    integrations.filter((item) => {
      const q = appSearchQuery.trim().toLowerCase();
      if (!q) {
        return !isSystemBackgroundHelper(item);
      }
      return (
        item.display_name.toLowerCase().includes(q) || item.bundle_id.toLowerCase().includes(q)
      );
    }),
  );

  function formatRuleDisplayName(key: string): string {
    return key
      .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
      .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2")
      .replace(/^A Ok\b/, "A-OK")
      .trim();
  }

  let enabledRulesCount = $derived(lintRules.filter((r) => r.enabled).length);
  let disabledRulesCount = $derived(lintRules.filter((r) => !r.enabled).length);

  let filteredDictionaryWords = $derived.by(() => {
    const q = newDictWord.trim().toLowerCase();
    if (!q) return dictionaryWords;
    return dictionaryWords.filter((w) => w.toLowerCase().includes(q));
  });

  let filteredRules = $derived(
    lintRules.filter((rule) => {
      if (ruleFilterMode === "enabled" && !rule.enabled) return false;
      if (ruleFilterMode === "disabled" && rule.enabled) return false;
      const q = ruleSearchQuery.trim().toLowerCase();
      if (!q) return true;
      const display = formatRuleDisplayName(rule.key).toLowerCase();
      return rule.key.toLowerCase().includes(q) || display.includes(q);
    }),
  );

  // ---------------------------------------------------------------------------
  // Lifecycle & Initialization
  // ---------------------------------------------------------------------------
  onMount(() => {
    isOverlayWindow = window.location.hash === "#elo-overlay";

    if (isOverlayWindow) {
      document.documentElement.classList.add("elo-overlay-mode");
      initOverlayMode();
    } else {
      document.documentElement.classList.remove("elo-overlay-mode");
      initStudioMode();
    }
  });

  // ---------------------------------------------------------------------------
  // Mode A: System-Wide "elo" Overlay Logic
  // ---------------------------------------------------------------------------
  let lastNativeClickAt = 0;
  let lastSyntheticClickAt = 0;
  let lastToggleCardAt = 0;
  let overlayPromptInputEl: HTMLInputElement | null = $state(null);
  let overlayInputFocused = $state(false);

  async function focusOverlayPromptInput() {
    if (!isOverlayWindow || rewriteLoading) return;
    overlayInputFocused = true;
    overlayPromptInputEl?.focus();
    try {
      await invoke("set_elo_overlay_input_focus", { focused: true });
    } catch {
      // Ignore
    }
    requestAnimationFrame(() => {
      overlayPromptInputEl?.focus();
    });
  }

  async function releaseOverlayPromptInput() {
    if (!overlayInputFocused) return;
    overlayInputFocused = false;
    overlayPromptInputEl?.blur();
    try {
      await invoke("set_elo_overlay_input_focus", { focused: false });
    } catch {
      // Ignore
    }
  }

  function initOverlayMode() {
    refreshOverlaySnapshot();

    const onCaptPointerUp = (e: PointerEvent) => {
      if (!e.isTrusted || e.button !== 0 || isDraggingElo || draggedDuringPress) {
        return;
      }
      const target = e.target as HTMLElement | null;
      if (!target) return;
      const inputEl = target.closest(
        ".elo-rewrite-card input.rewrite-prompt-input",
      ) as HTMLInputElement | null;
      if (inputEl && !inputEl.disabled) {
        focusOverlayPromptInput();
        return;
      }
      const btn = target.closest(
        ".elo-rewrite-card button, .elo-floating-card button, .elo-mini-card button",
      ) as HTMLButtonElement | null;
      if (!btn || btn.disabled) return;
      const upTime = Date.now();
      setTimeout(() => {
        if (lastNativeClickAt < upTime - 5 && lastSyntheticClickAt < upTime - 5) {
          lastSyntheticClickAt = Date.now();
          btn.click();
        }
      }, 32);
    };
    const onCaptClick = (e: MouseEvent) => {
      if (e.isTrusted) {
        const target = e.target as HTMLElement | null;
        if (target?.closest("input.rewrite-prompt-input")) {
          lastNativeClickAt = Date.now();
          focusOverlayPromptInput();
          return;
        }
        if (Date.now() - lastSyntheticClickAt < 350) {
          e.stopImmediatePropagation();
          e.preventDefault();
          return;
        }
        lastNativeClickAt = Date.now();
      }
    };
    window.addEventListener("pointerup", onCaptPointerUp, true);
    window.addEventListener("click", onCaptClick, true);

    const unlistenPromise = listen<EloOverlaySnapshot>("elo-overlay-snapshot", (event) => {
      handleIncomingSnapshot(event.payload);
    });

    const unlistenClickPromise = listen<{ x: number; y: number }>("elo-overlay-click", (event) => {
      if (isDraggingElo || draggedDuringPress) {
        return;
      }
      const { x, y } = event.payload;
      const hit = document.elementFromPoint(x, y) as HTMLElement | null;
      if (!hit) return;
      const inputHit = hit.closest("input.rewrite-prompt-input") as HTMLInputElement | null;
      if (inputHit && !inputHit.disabled) {
        focusOverlayPromptInput();
        return;
      }
      const now = Date.now();
      if (now - lastNativeClickAt < 350 || now - lastSyntheticClickAt < 350) {
        return;
      }
      lastSyntheticClickAt = now;
      const btn = hit.closest("button, input, a, [role='button']") as HTMLElement | null;
      if (btn) {
        if (btn.tagName === "INPUT") {
          focusOverlayPromptInput();
        } else {
          btn.click();
        }
      } else if (hit.closest(".elo-teardrop-widget")) {
        toggleFullEloCard();
      }
    });

    const unlistenOutsidePromise = listen("elo-overlay-outside-click", () => {
      releaseOverlayPromptInput();
      overlayPopoverMode = "closed";
      overlayMoreMenuOpen = false;
      eloPowerMenuOpen = false;
      syncOverlayRegions();
    });

    const unlistenInputReadyPromise = listen("elo-overlay-input-ready", () => {
      if (overlayInputFocused && overlayPromptInputEl) {
        overlayPromptInputEl.focus();
      }
    });

    const unlistenRewriteStatePromise = listen<{ active: boolean }>(
      "elo-rewrite-state",
      (event) => {
        rewriteLoading = Boolean(event.payload?.active);
      },
    );

    const pollTimer = setInterval(refreshOverlaySnapshot, 180);

    return () => {
      clearInterval(pollTimer);
      window.removeEventListener("pointerup", onCaptPointerUp, true);
      window.removeEventListener("click", onCaptClick, true);
      unlistenPromise.then((unlisten) => unlisten());
      unlistenClickPromise.then((unlisten) => unlisten());
      unlistenOutsidePromise.then((unlisten) => unlisten());
      unlistenInputReadyPromise.then((unlisten) => unlisten());
      unlistenRewriteStatePromise.then((unlisten) => unlisten());
    };
  }

  async function refreshOverlaySnapshot() {
    try {
      const snap = await invoke<EloOverlaySnapshot>("get_elo_overlay_snapshot");
      handleIncomingSnapshot(snap);
    } catch {
      // Ignore transient IPC errors
    }
  }

  function handleIncomingSnapshot(snap: EloOverlaySnapshot) {
    const prevBundle = overlaySnap.app_bundle_id;
    overlaySnap = snap;

    if (!snap.field_active) {
      overlayPopoverMode = "closed";
      overlayMoreMenuOpen = false;
      eloPillHovered = false;
      eloPowerMenuOpen = false;
    } else if (prevBundle !== snap.app_bundle_id) {
      dragCurrentOffset = { x: 0, y: 0 };
      overlayPopoverMode = "closed";
    }

    if (overlayActiveAlertId && !snap.alerts.some((a) => a.id === overlayActiveAlertId)) {
      overlayActiveAlertId = snap.alerts[0]?.id ?? null;
      if (overlayPopoverMode === "mini" && !overlayActiveAlertId) {
        overlayPopoverMode = "closed";
      }
    }

    if (snap.card_command) {
      handleOverlayCardCommand(snap.card_command);
    }

    syncOverlayRegions();
  }

  function handleOverlayCardCommand(cmd: string) {
    if (cmd === "focus") {
      overlayPopoverMode = "full";
      if (!overlayActiveAlertId && overlaySnap.alerts.length > 0) {
        overlayActiveAlertId = overlaySnap.alerts[0].id;
      }
    } else if (cmd === "close") {
      overlayPopoverMode = "closed";
    } else if (cmd === "next" && overlaySnap.alerts.length > 0) {
      if (overlayPopoverMode === "closed") overlayPopoverMode = "full";
      const nextIdx = (activeOverlayAlertIndex + 1) % overlaySnap.alerts.length;
      overlayActiveAlertId = overlaySnap.alerts[nextIdx].id;
    } else if (cmd === "prev" && overlaySnap.alerts.length > 0) {
      if (overlayPopoverMode === "closed") overlayPopoverMode = "full";
      const prevIdx =
        (activeOverlayAlertIndex - 1 + overlaySnap.alerts.length) % overlaySnap.alerts.length;
      overlayActiveAlertId = overlaySnap.alerts[prevIdx].id;
    } else if (cmd === "accept" && activeOverlayAlert) {
      applyOverlaySuggestion(activeOverlayAlert.id, 0);
    } else if (cmd === "dismiss" && activeOverlayAlert) {
      dismissOverlayAlert(activeOverlayAlert.id);
    }
  }

  async function syncOverlayRegions() {
    if (!isOverlayWindow) return;
    try {
      await invoke("set_elo_overlay_regions", {
        widgetRect: effectiveWidgetRect,
        cardRect: effectiveCardRect,
        isDragging: isDraggingElo,
      });
    } catch {
      // Ignore
    }
  }

  $effect(() => {
    if (isOverlayWindow) {
      const _w = effectiveWidgetRect;
      const _c = effectiveCardRect;
      const _d = isDraggingElo;
      void _w;
      void _c;
      void _d;
      syncOverlayRegions();
      if (overlayPopoverMode !== "rewrite" && overlayInputFocused) {
        releaseOverlayPromptInput();
      }
    }
  });

  let pointerDownOnElo = $state(false);

  function toggleFullEloCard(e?: MouseEvent) {
    if (e) e.stopPropagation();
    if (draggedDuringPress) return;
    const now = Date.now();
    if (now - lastToggleCardAt < 220) return;
    lastToggleCardAt = now;
    overlayPopoverMode = overlayPopoverMode === "full" ? "closed" : "full";
    overlayMoreMenuOpen = false;
    eloPowerMenuOpen = false;
    if (overlayPopoverMode === "full" && !overlayActiveAlertId && overlaySnap.alerts[0]) {
      overlayActiveAlertId = overlaySnap.alerts[0].id;
    }
    syncOverlayRegions();
  }

  function onEloWidgetPointerDown(e: PointerEvent) {
    if (e.button !== 0 || !baseWidgetRect) return;
    const targetEl = e.target as HTMLElement;
    if (targetEl.closest(".elo-sub-action-btn") || targetEl.closest(".elo-power-menu")) {
      return;
    }
    pointerDownOnElo = true;
    draggedDuringPress = false;
    dragStartPointer = { x: e.clientX, y: e.clientY };
    dragStartWidget = {
      x: baseWidgetRect.x,
      y: baseWidgetRect.y,
    };
    dragCurrentOffset = { x: 0, y: 0 };
  }

  function onEloWidgetPointerMove(e: PointerEvent) {
    if (!pointerDownOnElo) return;
    const dx = e.clientX - dragStartPointer.x;
    const dy = e.clientY - dragStartPointer.y;
    if (!isDraggingElo && (Math.abs(dx) > 5 || Math.abs(dy) > 5)) {
      isDraggingElo = true;
      draggedDuringPress = true;
      const current = e.currentTarget as HTMLElement;
      current.setPointerCapture(e.pointerId);
      syncOverlayRegions();
    }
    if (isDraggingElo) {
      dragCurrentOffset = { x: dx, y: dy };
    }
  }

  async function onEloWidgetPointerUp(e: PointerEvent) {
    if (!pointerDownOnElo) return;
    pointerDownOnElo = false;
    const current = e.currentTarget as HTMLElement;
    if (current.hasPointerCapture(e.pointerId)) {
      current.releasePointerCapture(e.pointerId);
    }
    const wasDragging = isDraggingElo && draggedDuringPress;
    isDraggingElo = false;

    if (wasDragging && overlaySnap.field_rect) {
      const w = 104;
      const h = 38;
      const fr = overlaySnap.field_rect;
      const baseX = Math.max(fr.x + 6, fr.x + fr.width - w - 8);
      const baseY = fr.height < 56 ? fr.y + fr.height + 4 : fr.y + fr.height - h - 8;
      const finalX = dragStartWidget.x + dragCurrentOffset.x;
      const finalY = dragStartWidget.y + dragCurrentOffset.y;
      await invoke("save_elo_widget_offset", {
        bundleId: overlaySnap.app_bundle_id || "default",
        offsetX: finalX - baseX,
        offsetY: finalY - baseY,
      });
      await refreshOverlaySnapshot();
    }
    dragCurrentOffset = { x: 0, y: 0 };
    syncOverlayRegions();
  }

  function openRewritePopover(e: MouseEvent) {
    e.stopPropagation();
    const now = Date.now();
    if (now - lastToggleCardAt < 220) return;
    lastToggleCardAt = now;
    eloPowerMenuOpen = false;
    rewriteError = null;
    rewritePreview = null;
    overlayPopoverMode = overlayPopoverMode === "rewrite" ? "closed" : "rewrite";
    syncOverlayRegions();
  }

  async function triggerOverlayRewrite(mode: string, customPrompt?: string) {
    await releaseOverlayPromptInput();
    rewriteLoading = true;
    rewriteError = null;
    const startedAt = Date.now();
    try {
      const resp = await invoke<EloRewriteResponse>("elo_rewrite_focused_text", {
        mode,
        customPrompt: customPrompt ?? null,
        sourceOverride: null,
      });
      const elapsed = Date.now() - startedAt;
      if (elapsed < 450) {
        await new Promise((r) => setTimeout(r, 450 - elapsed));
      }
      rewritePreview = resp;
      rewriteHistory = [resp, ...rewriteHistory.slice(0, 4)];
    } catch (err) {
      rewriteError = String(err);
    } finally {
      rewriteLoading = false;
    }
  }

  async function applyOverlayRewriteToApp() {
    if (!rewritePreview) return;
    const newText = rewritePreview.rewritten;
    rewritePreview = null;
    overlayPopoverMode = "closed";
    syncOverlayRegions();
    try {
      await invoke("apply_elo_field_rewrite", { text: newText });
      await refreshOverlaySnapshot();
    } catch {
      // Ignore
    }
  }

  async function pauseEloForCurrentApp() {
    eloPowerMenuOpen = false;
    overlayPopoverMode = "closed";
    if (overlaySnap.app_bundle_id) {
      await invoke("set_integration_enabled", {
        bundleId: overlaySnap.app_bundle_id,
        enabled: false,
      });
    } else {
      await invoke("stop_highlighter_service");
    }
    await refreshOverlaySnapshot();
  }

  // Hovering or clicking an inline word underline opens the Compact Quick-Fix Mini-Popover (Image 3)
  function onUnderlineClick(alert: EloAlert) {
    overlayActiveAlertId = alert.id;
    overlayPopoverMode = "mini";
    overlayMoreMenuOpen = false;
    syncOverlayRegions();
  }

  function onUnderlineHover(alert: EloAlert) {
    overlayHoveredAlertId = alert.id;
    if (overlayPopoverMode === "closed" || overlayPopoverMode === "mini") {
      overlayActiveAlertId = alert.id;
      overlayPopoverMode = "mini";
      syncOverlayRegions();
    }
  }

  let lastAppliedAlertKey = "";
  let lastAppliedAlertAt = 0;

  async function applyOverlaySuggestion(alertId: string, suggestionIndex = 0) {
    const now = Date.now();
    const key = `${alertId}:${suggestionIndex}`;
    if (key === lastAppliedAlertKey && now - lastAppliedAlertAt < 500) {
      return;
    }
    lastAppliedAlertKey = key;
    lastAppliedAlertAt = now;

    overlayMoreMenuOpen = false;
    overlaySnap.alerts = overlaySnap.alerts.filter((a) => a.id !== alertId);
    if (overlayPopoverMode === "mini" || overlaySnap.alerts.length === 0) {
      overlayPopoverMode = "closed";
    } else if (overlayActiveAlertId === alertId) {
      overlayActiveAlertId = overlaySnap.alerts[0]?.id ?? null;
    }
    syncOverlayRegions();

    try {
      await invoke("apply_elo_alert", { alertId, suggestionIndex });
      await refreshOverlaySnapshot();
    } catch {
      // Ignore
    }
  }

  async function dismissOverlayAlert(alertId: string) {
    overlayMoreMenuOpen = false;
    if (overlayPopoverMode === "mini") {
      overlayPopoverMode = "closed";
    }
    try {
      await invoke("dismiss_elo_alert", { alertId });
      await refreshOverlaySnapshot();
    } catch {
      // Ignore
    }
  }

  async function addOverlayWordToDictionary(word: string, alertId: string) {
    overlayMoreMenuOpen = false;
    try {
      await invoke("add_to_dictionary", { word });
      await invoke("dismiss_elo_alert", { alertId });
      await refreshOverlaySnapshot();
    } catch {
      // Ignore
    }
  }

  async function disableOverlayRule(ruleName: string, alertId: string) {
    overlayMoreMenuOpen = false;
    try {
      await invoke("disable_elo_rule", { ruleName });
      await invoke("dismiss_elo_alert", { alertId });
      await refreshOverlaySnapshot();
    } catch {
      // Ignore
    }
  }

  async function openStudioFromOverlay(tab: StudioTab, importFocusedText = false) {
    overlayPopoverMode = "closed";
    syncOverlayRegions();
    try {
      await invoke("open_elocutio_studio", {
        tab,
        importFocusedText,
      });
    } catch {
      // Ignore
    }
  }

  // ---------------------------------------------------------------------------
  // Mode B: Desktop Studio & Settings Logic
  // ---------------------------------------------------------------------------
  async function initStudioMode() {
    await runEditorCheck();
    await loadAllSettings();
    await checkStudioLaunchRequest();

    const launchPoll = setInterval(checkStudioLaunchRequest, 600);
    return () => clearInterval(launchPoll);
  }

  async function checkStudioLaunchRequest() {
    try {
      const state = await invoke<{
        requested_tab: string | null;
        imported_text: string | null;
        imported_app_name: string | null;
      }>("take_studio_launch_state");

      if (state.imported_text && state.imported_text.trim().length > 0) {
        docTitle = state.imported_app_name
          ? `Draft from ${state.imported_app_name}`
          : "Imported Draft";
        editorText = state.imported_text;
        await runEditorCheck();
      }
      if (
        state.requested_tab &&
        ["editor", "apps", "rules", "voice", "ai"].includes(state.requested_tab)
      ) {
        activeTab = state.requested_tab as StudioTab;
      }
    } catch {
      // Ignore
    }
  }

  let lintDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let studioAiDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let lastStudioAiAnalyzedText = "";

  function scheduleEditorCheck() {
    if (lintDebounceTimer) clearTimeout(lintDebounceTimer);
    lintDebounceTimer = setTimeout(() => {
      runEditorCheck();
    }, 90);
  }

  function scheduleStudioAiAnalysis(snapshotText: string) {
    if (studioAiDebounceTimer) clearTimeout(studioAiDebounceTimer);
    const trimmed = snapshotText.trim();
    if (!trimmed || localOnlyMode || trimmed === lastStudioAiAnalyzedText) {
      return;
    }
    studioAiDebounceTimer = setTimeout(async () => {
      const current = editorText.trim();
      if (!current || localOnlyMode || current !== trimmed) return;
      lastStudioAiAnalyzedText = current;
      try {
        const updated = await invoke<boolean>("analyze_studio_text", {
          text: editorText,
        });
        if (updated && editorText.trim() === current) {
          const items = await invoke<LocalLintItem[]>("check_text_local", {
            text: editorText,
          });
          editorLints = items;
          if (!editorSelectedLintId && items.length > 0) {
            editorSelectedLintId = items[0].id;
          }
        }
      } catch {
        // Ignore AI background errors; local Harper suggestions stay active
      }
    }, 900);
  }

  async function runEditorCheck() {
    try {
      const items = await invoke<LocalLintItem[]>("check_text_local", {
        text: editorText,
      });
      editorLints = items;
      if (editorSelectedLintId && !items.some((i) => i.id === editorSelectedLintId)) {
        editorSelectedLintId = items[0]?.id ?? null;
        if (editorInlineCardMode === "mini") editorInlineCardPos = null;
      } else if (!editorSelectedLintId && items.length > 0) {
        editorSelectedLintId = items[0].id;
      }

      if (editorText.trim().length > 0) {
        const toneResp = await invoke<{
          label: string;
          confidence: number;
        }>("detect_text_tone", { text: editorText });
        editorTone = toneResp.label;
        scheduleStudioAiAnalysis(editorText);
      } else {
        editorTone = "Neutral";
      }
    } catch {
      // Ignore
    }
  }

  function syncScroll() {
    if (textareaEl && backdropEl) {
      backdropEl.scrollTop = textareaEl.scrollTop;
      backdropEl.scrollLeft = textareaEl.scrollLeft;
    }
  }

  function handleTextareaClick(e: MouseEvent) {
    if (!textareaEl) return;
    const caret = textareaEl.selectionStart ?? 0;
    const hit = editorLints.find((l) => caret >= l.start && caret <= l.end);
    if (hit) {
      editorSelectedLintId = hit.id;
      editorInlineCardMode = "mini";
      const rect = textareaEl.getBoundingClientRect();
      editorInlineCardPos = {
        x: Math.max(24, Math.min(e.clientX - rect.left, rect.width - 280)),
        y: Math.max(36, Math.min(e.clientY - rect.top + 18, rect.height - 190)),
      };
    } else if (editorInlineCardMode === "mini") {
      editorInlineCardPos = null;
    }
  }

  function selectAlertFromFeed(lint: LocalLintItem) {
    editorSelectedLintId = lint.id;
    editorInlineCardPos = null;
    if (textareaEl) {
      textareaEl.focus();
      textareaEl.setSelectionRange(lint.start, lint.end);
    }
  }

  function applyEditorSuggestion(lint: LocalLintItem, replacement: string) {
    const chars = Array.from(editorText);
    const before = chars.slice(0, lint.start).join("");
    const after = chars.slice(lint.end).join("");
    editorText = `${before}${replacement}${after}`;
    editorInlineCardPos = null;
    runEditorCheck();
    invoke<PersonalizationDbStatus>("record_studio_edit_feedback", {
      kind: "accepted",
      category: lint.category_label || "Grammar",
      original: lint.original,
      replacement,
    })
      .then((status) => {
        personalizationDb = status;
      })
      .catch(() => {});
  }

  function acceptAllEditorSuggestions() {
    const list =
      editorCategoryFilter === "all"
        ? [...editorLints]
        : editorLints.filter((l) => l.category === editorCategoryFilter);
    if (list.length === 0) return;

    const sortedDesc = [...list].sort((a, b) => b.start - a.start);
    let chars = Array.from(editorText);
    for (const lint of sortedDesc) {
      const first = lint.suggestions[0];
      if (!first) continue;
      const replChars = Array.from(first.text);
      chars.splice(lint.start, lint.end - lint.start, ...replChars);
      invoke<PersonalizationDbStatus>("record_studio_edit_feedback", {
        kind: "accepted",
        category: lint.category_label || "Grammar",
        original: lint.original,
        replacement: first.text,
      })
        .then((status) => {
          personalizationDb = status;
        })
        .catch(() => {});
    }
    editorText = chars.join("");
    editorInlineCardPos = null;
    runEditorCheck();
  }

  function dismissEditorLint(lintId: string) {
    const target = editorLints.find((l) => l.id === lintId);
    if (target) {
      invoke<PersonalizationDbStatus>("record_studio_edit_feedback", {
        kind: "dismissed",
        category: target.category_label || "Grammar",
        original: target.original,
        replacement: target.suggestions[0]?.text ?? "",
      })
        .then((status) => {
          personalizationDb = status;
        })
        .catch(() => {});
    }
    editorLints = editorLints.filter((l) => l.id !== lintId);
    if (editorSelectedLintId === lintId) {
      editorSelectedLintId = editorLints[0]?.id ?? null;
      editorInlineCardPos = null;
    }
  }

  async function addEditorWordToDictionary(word: string, lintId: string) {
    if (!canAddToLexicon(word)) return;
    try {
      await invoke("add_to_dictionary", { word: word.trim() });
      dictionaryWords = await invoke<string[]>("get_dictionary");
      dismissEditorLint(lintId);
      await runEditorCheck();
    } catch {
      // Ignore
    }
  }

  function canAddToLexicon(original: string, subcategory?: string): boolean {
    const trimmed = (original || "").trim();
    if (!trimmed || trimmed.length < 2 || trimmed.length > 48) return false;
    if (/\s/.test(trimmed)) return false;
    const sub = (subcategory || "").toLowerCase();
    if (
      sub.includes("readability") ||
      sub.includes("punct") ||
      sub.includes("comma") ||
      sub.includes("sentence")
    ) {
      return false;
    }
    return true;
  }

  let rewritingLintId = $state<string | null>(null);

  async function rewriteSingleSentenceLint(lint: LocalLintItem) {
    if (rewritingLintId) return;
    rewritingLintId = lint.id;
    try {
      const resp = await invoke<EloRewriteResponse>("elo_rewrite_focused_text", {
        mode: "concise",
        customPrompt:
          "Split or tighten this long sentence so it reads clearly and concisely while preserving its meaning.",
        sourceOverride: lint.original,
      });
      if (resp.rewritten && resp.rewritten.trim()) {
        applyEditorSuggestion(lint, resp.rewritten.trim());
      }
    } catch {
      // Ignore
    } finally {
      rewritingLintId = null;
    }
  }

  async function disableEditorRule(ruleName: string, lintId: string) {
    try {
      await invoke("disable_elo_rule", { ruleName });
      lintRules = await invoke<LintRuleView[]>("get_lint_rules");
      dismissEditorLint(lintId);
      await runEditorCheck();
    } catch {
      // Ignore
    }
  }

  function insertMarkdownWrap(prefix: string, suffix: string) {
    if (!textareaEl) return;
    const start = textareaEl.selectionStart ?? editorText.length;
    const end = textareaEl.selectionEnd ?? editorText.length;
    const selected = editorText.slice(start, end) || "text";
    editorText = editorText.slice(0, start) + prefix + selected + suffix + editorText.slice(end);
    scheduleEditorCheck();
  }

  // Draggable Studio inline "elo" widget handlers
  function onStudioEloPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    const targetEl = e.target as HTMLElement;
    if (targetEl.closest(".elo-sub-action-btn")) return;
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    studioEloDragging = true;
    studioEloMoved = false;
    studioEloDragStart = {
      px: e.clientX,
      py: e.clientY,
      ox: studioEloOffset.x,
      oy: studioEloOffset.y,
    };
  }

  function onStudioEloPointerMove(e: PointerEvent) {
    if (!studioEloDragging) return;
    const dx = e.clientX - studioEloDragStart.px;
    const dy = e.clientY - studioEloDragStart.py;
    if (Math.abs(dx) > 4 || Math.abs(dy) > 4) {
      studioEloMoved = true;
    }
    studioEloOffset = {
      x: Math.max(-420, Math.min(20, studioEloDragStart.ox + dx)),
      y: Math.max(-460, Math.min(20, studioEloDragStart.oy + dy)),
    };
  }

  function onStudioEloPointerUp(e: PointerEvent) {
    if (!studioEloDragging) return;
    const target = e.currentTarget as HTMLElement;
    if (target.hasPointerCapture(e.pointerId)) {
      target.releasePointerCapture(e.pointerId);
    }
    studioEloDragging = false;
    if (!studioEloMoved && editorLints.length > 0) {
      const idx = editorLints.findIndex((l) => l.id === editorSelectedLintId);
      const next = editorLints[(idx + 1) % editorLints.length];
      if (next) {
        selectAlertFromFeed(next);
        editorInlineCardMode = "mini";
        editorInlineCardPos = { x: 48, y: 84 };
      }
    }
  }

  function openStudioRewriteCard(e: MouseEvent) {
    e.stopPropagation();
    aiRewriteError = null;
    aiRewriteResult = null;
    editorInlineCardMode = "rewrite";
    editorInlineCardPos =
      editorInlineCardPos && editorInlineCardMode === "rewrite" ? null : { x: 120, y: 90 };
  }

  async function runStudioIdeaRewrite(mode: string, customPrompt?: string) {
    if (!editorText.trim()) return;
    aiRewriteLoading = true;
    aiRewriteError = null;
    aiRewriteResult = null;
    const startedAt = Date.now();
    try {
      const resp = await invoke<EloRewriteResponse>("elo_rewrite_focused_text", {
        mode,
        customPrompt: customPrompt ?? null,
        sourceOverride: editorText,
      });
      const elapsed = Date.now() - startedAt;
      if (elapsed < 450) {
        await new Promise((r) => setTimeout(r, 450 - elapsed));
      }
      aiRewriteResult = resp.rewritten;
    } catch (err) {
      aiRewriteError = String(err);
    } finally {
      aiRewriteLoading = false;
    }
  }

  // ---------------------------------------------------------------------------
  // Settings Loaders & Mutators
  // ---------------------------------------------------------------------------
  async function loadAllSettings() {
    try {
      axStatus = await invoke<string>("get_accessibility_permission_status");
      serviceRunning = await invoke<boolean>("get_highlighter_service_status");
      autoEnableNewApps = await invoke<boolean>("get_auto_enable_new_apps");
      integrations = await invoke<IntegrationItem[]>("get_integrations");
      dialectName = await invoke<string>("get_dialect_name");
      dictionaryWords = await invoke<string[]>("get_dictionary");
      lintRules = await invoke<LintRuleView[]>("get_lint_rules");
      voiceTraits = await invoke<VoiceTraitView[]>("get_voice_traits");
      learnFromEdits = await invoke<boolean>("get_learn_from_edits");
      localOnlyMode = await invoke<boolean>("get_local_only");
      providerSettings = await invoke<ProviderSettings>("get_provider_settings");
      customInstruction = await invoke<string>("get_custom_instruction");
      const savedBudget = await invoke<number | null>("get_budget");
      budgetInput = savedBudget !== null ? String(savedBudget) : "";
      const savedCap = await invoke<number | null>("get_cost_cap");
      costCapInput = savedCap !== null ? String(savedCap) : "";
      usageSummary = await invoke<UsageSummaryView>("get_usage_summary", {
        limit: 20,
      });
      selectedPresetId = detectPresetFromUrl(providerSettings.kind, providerSettings.base_url);
      refreshPresetModels(selectedPresetId, providerSettings.base_url);
      personalizationDb = await invoke<PersonalizationDbStatus>("get_personalization_db_status");
      writingAnalytics = await invoke<WritingAnalyticsSnapshot>("get_writing_analytics");
    } catch {
      // Ignore
    }
  }

  function detectPresetFromUrl(kind: string, baseUrl: string): string {
    const lower = (baseUrl || "").toLowerCase();
    if (kind === "open_router" || lower.includes("openrouter.ai")) {
      return "openrouter";
    }
    if (lower.includes("localhost:11434") || lower.includes("127.0.0.1:11434")) {
      return "ollama_local";
    }
    if (lower.includes("ollama.com")) {
      return "ollama_cloud";
    }
    if (lower.includes("11535") || lower.includes("apple")) {
      return "apple_foundation";
    }
    if (lower.includes("api.openai.com")) {
      return "openai";
    }
    if (lower.includes("groq.com")) {
      return "groq";
    }
    return "openrouter";
  }

  async function refreshPresetModels(presetId: string, baseUrl: string) {
    fetchingModels = true;
    try {
      presetModelSlugs = await invoke<PresetModelSlug[]>("fetch_provider_preset_models", {
        presetId,
        baseUrl,
      });
    } catch {
      presetModelSlugs = [];
    } finally {
      fetchingModels = false;
    }
  }

  async function selectProviderPreset(preset: ProviderPresetDef) {
    selectedPresetId = preset.id;
    providerSettings.kind = preset.kind;
    providerSettings.base_url = preset.baseUrl;
    providerStatusMsg = null;
    customModelSlugMode = false;
    await refreshPresetModels(preset.id, preset.baseUrl);
    if (
      presetModelSlugs.length > 0 &&
      !presetModelSlugs.some((m) => m.slug === providerSettings.model)
    ) {
      providerSettings.model = presetModelSlugs[0].slug;
    } else if (!providerSettings.model.trim()) {
      providerSettings.model = preset.defaultModel;
    }
    try {
      await invoke("set_provider_kind", { kind: preset.kind });
      await invoke("set_provider_base_url", { baseUrl: preset.baseUrl });
      await invoke("set_provider_model", { model: providerSettings.model });
      providerSettings = await invoke<ProviderSettings>("get_provider_settings");
    } catch {
      // Ignore
    }
  }

  async function onPresetSelectChange(e: Event) {
    const id = (e.currentTarget as HTMLSelectElement).value;
    const preset = PROVIDER_PRESETS.find((p) => p.id === id);
    if (preset) {
      await selectProviderPreset(preset);
    } else {
      selectedPresetId = "custom";
      providerSettings.kind = "open_ai_compatible";
      customModelSlugMode = true;
    }
  }

  function onModelSlugSelectChange(e: Event) {
    const val = (e.currentTarget as HTMLSelectElement).value;
    if (val === "__custom__") {
      customModelSlugMode = true;
    } else {
      customModelSlugMode = false;
      providerSettings.model = val;
    }
  }

  async function toggleEloService() {
    try {
      if (serviceRunning) {
        serviceRunning = await invoke<boolean>("stop_highlighter_service");
      } else {
        serviceRunning = await invoke<boolean>("start_highlighter_service");
      }
    } catch {
      // Ignore
    }
  }

  async function handleGrantAccessibility() {
    try {
      axStatus = await invoke<string>("reset_and_request_accessibility");
    } catch {
      // Ignore
    }
  }

  async function toggleAppIntegration(bundleId: string, enabled: boolean) {
    try {
      await invoke("set_integration_enabled", { bundleId, enabled });
      integrations = await invoke<IntegrationItem[]>("get_integrations");
    } catch {
      // Ignore
    }
  }

  async function toggleAutoEnableApps() {
    try {
      const next = !autoEnableNewApps;
      await invoke("set_auto_enable_new_apps", { autoEnableNewApps: next });
      autoEnableNewApps = next;
    } catch {
      // Ignore
    }
  }

  async function changeDialect(abbr: string) {
    try {
      dialectName = await invoke<string>("set_dialect_name", { dialect: abbr });
      personalizationDb = await invoke<PersonalizationDbStatus>("get_personalization_db_status");
      await runEditorCheck();
    } catch {
      // Ignore
    }
  }

  async function toggleLintRule(ruleKey: string, enabled: boolean) {
    lintRules = lintRules.map((r) => (r.key === ruleKey ? { ...r, enabled } : r));
    try {
      await invoke("set_lint_rule_enabled", {
        ruleName: ruleKey,
        enabled,
      });
      personalizationDb = await invoke<PersonalizationDbStatus>("get_personalization_db_status");
      await runEditorCheck();
    } catch {
      lintRules = await invoke<LintRuleView[]>("get_lint_rules");
    }
  }

  async function addNewDictionaryWord() {
    const w = newDictWord.trim();
    if (!w) return;
    try {
      await invoke("add_to_dictionary", { word: w });
      newDictWord = "";
      dictionaryWords = await invoke<string[]>("get_dictionary");
      personalizationDb = await invoke<PersonalizationDbStatus>("get_personalization_db_status");
      await runEditorCheck();
    } catch {
      // Ignore
    }
  }

  async function removeDictionaryWord(word: string) {
    try {
      const updated = dictionaryWords.filter((w) => w !== word);
      await invoke("set_dictionary", { words: updated });
      dictionaryWords = await invoke<string[]>("get_dictionary");
      personalizationDb = await invoke<PersonalizationDbStatus>("get_personalization_db_status");
      await runEditorCheck();
    } catch {
      // Ignore
    }
  }

  async function handleLearnVoice() {
    if (!voiceSampleText.trim()) return;
    voiceStatusMsg = null;
    try {
      await invoke("learn_voice_samples", {
        text: voiceSampleText,
        name: "My Writing Voice",
      });
      voiceTraits = await invoke<VoiceTraitView[]>("get_voice_traits");
      personalizationDb = await invoke<PersonalizationDbStatus>("get_personalization_db_status");
      voiceStatusMsg = "Learned voice profile from your writing sample.";
    } catch (err) {
      voiceStatusMsg = String(err);
    }
  }

  async function toggleLocalOnly() {
    try {
      localOnlyMode = await invoke<boolean>("set_local_only", {
        enabled: !localOnlyMode,
      });
    } catch {
      // Ignore
    }
  }

  async function saveProviderSettings() {
    providerStatusMsg = null;
    try {
      await invoke("set_provider_kind", { kind: providerSettings.kind });
      await invoke("set_provider_base_url", {
        baseUrl: providerSettings.base_url,
      });
      await invoke("set_provider_model", { model: providerSettings.model });
      await invoke("set_custom_instruction", {
        instruction: customInstruction,
      });
      const parsedBudget = budgetInput.trim() === "" ? null : Number(budgetInput.trim());
      await invoke("set_budget", {
        budgetUsd: parsedBudget !== null && Number.isFinite(parsedBudget) ? parsedBudget : null,
      });
      const parsedCap = costCapInput.trim() === "" ? null : Number(costCapInput.trim());
      await invoke("set_cost_cap", {
        costCapUsd: parsedCap !== null && Number.isFinite(parsedCap) ? parsedCap : null,
      });
      if (apiKeyInput.trim()) {
        await invoke("set_openrouter_key", { key: apiKeyInput.trim() });
        apiKeyInput = "";
      }
      providerSettings = await invoke<ProviderSettings>("get_provider_settings");
      usageSummary = await invoke<UsageSummaryView>("get_usage_summary", {
        limit: 20,
      });
      personalizationDb = await invoke<PersonalizationDbStatus>("get_personalization_db_status");
      providerStatusIsError = false;
      providerStatusMsg = "Saved provider configuration, budget, and AES-256 encrypted key.";
    } catch (err) {
      providerStatusIsError = true;
      providerStatusMsg = String(err);
    }
  }

  async function testProviderConnection() {
    providerStatusIsError = false;
    providerStatusMsg = "Testing provider connection…";
    try {
      await saveProviderSettings();
      const status = await invoke<{
        label: string | null;
        usage: number | null;
        limit: number | null;
        is_free_tier: boolean;
      }>("test_openrouter_key");
      const label = status.label ? ` (${status.label})` : "";
      providerStatusIsError = false;
      providerStatusMsg = `Connection verified${label}.`;
    } catch (err) {
      providerStatusIsError = true;
      providerStatusMsg = String(err);
    }
  }
</script>

<!-- =========================================================================
     REUSABLE SNIPPETS:
     1. "THE EDITORIAL WINK" BRAND LOGO SVG (Brand Guide v1.1)
     2. SPARKLE LIGHTBULB ICON SVG ("Rewrite with elo", Images 1, 2, 4)
     ========================================================================= -->
{#snippet editorialWinkLogo(size = 22, bg = "#191F70", fg = "#FFFFFF")}
  {#if bg === "transparent" || bg === "none"}
    <img
      src={fg === "#171717" || fg === "#000000"
        ? "/mark-black.png"
        : fg === "#FFFFFF"
          ? "/mark-white.png"
          : "/mark-indigo.png"}
      alt="elo"
      width={size}
      height={size}
      class="elo-brand-mark-img"
      draggable="false"
    />
  {:else}
    <span
      class="elo-brand-badge"
      style="
        width: {size}px;
        height: {size}px;
        background: {bg};
        border-radius: {Math.round(size * 0.3)}px;
      "
      aria-hidden="true"
    >
      <img
        src={fg === "#191F70"
          ? "/mark-indigo.png"
          : fg === "#171717" || fg === "#000000"
            ? "/mark-black.png"
            : "/mark-white.png"}
        alt=""
        style="width: {Math.round(size * 0.68)}px; height: {Math.round(size * 0.68)}px;"
        class="elo-brand-mark-img"
        draggable="false"
      />
    </span>
  {/if}
{/snippet}

{#snippet sparkleBulbIcon(size = 20)}
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
    style="flex-shrink: 0;"
  >
    <!-- Bulb body -->
    <path
      d="M12 2C7.86 2 4.5 5.36 4.5 9.5C4.5 12.12 5.84 14.42 7.88 15.74C8.42 16.09 8.75 16.7 8.75 17.35V18C8.75 18.55 9.2 19 9.75 19H14.25C14.8 19 15.25 18.55 15.25 18V17.35C15.25 16.7 15.58 16.09 16.12 15.74C18.16 14.42 19.5 12.12 19.5 9.5C19.5 5.36 16.14 2 12 2Z"
      fill="#10B981"
    />
    <!-- Bulb base -->
    <rect x="9.2" y="20.2" width="5.6" height="2" rx="1" fill="#0F7B55" />
    <!-- 4-point Sparkle star inside bulb -->
    <path
      d="M12 5.4L13.15 8.35L16.1 9.5L13.15 10.65L12 13.6L10.85 10.65L7.9 9.5L10.85 8.35L12 5.4Z"
      fill="#FFFFFF"
    />
  </svg>
{/snippet}

{#if isOverlayWindow}
  <!-- =======================================================================
       MODE A: SYSTEM-WIDE "elo" INLINE OVERLAY WINDOW
       ======================================================================= -->
  <div class="elo-overlay-stage">
    {#if overlaySnap.service_enabled && overlaySnap.field_active && overlaySnap.field_rect}
      <!-- 1. Clipped Field Stage: Underlines never bleed outside the focused text area -->
      <div
        class="elo-field-clip"
        style="
          left: {overlaySnap.field_rect.x}px;
          top: {overlaySnap.field_rect.y}px;
          width: {overlaySnap.field_rect.width}px;
          height: {overlaySnap.field_rect.height}px;
        "
      >
        {#each overlaySnap.alerts as alert (alert.id)}
          {@const isActive = overlayPopoverMode !== "closed" && activeOverlayAlert?.id === alert.id}
          {@const isHovered = overlayHoveredAlertId === alert.id}
          <button
            type="button"
            class="elo-word-underline"
            class:is-active={isActive || isHovered}
            style="
              left: {alert.rect.x - overlaySnap.field_rect.x}px;
              top: {alert.rect.y - overlaySnap.field_rect.y}px;
              width: {Math.max(10, alert.rect.width)}px;
              height: {Math.max(12, alert.rect.height)}px;
              --underline-color: {alert.color};
            "
            aria-label="{alert.category_label}: {alert.original}"
            onmouseenter={() => onUnderlineHover(alert)}
            onmouseleave={() => (overlayHoveredAlertId = null)}
            onclick={() => onUnderlineClick(alert)}
          ></button>
        {/each}
      </div>

      <!-- 2. Teardrop "elo" Floating Widget (Images 1, 2, 4: Resting [💡✨ | 2] -> Hover [⏻ | 📢 | 💡✨ | 2]) -->
      {#if baseWidgetRect}
        <div
          role="group"
          aria-label="elo floating writing widget"
          class="elo-teardrop-widget"
          class:is-expanded={eloPillHovered || eloPowerMenuOpen}
          class:is-dragging={isDraggingElo}
          class:is-rewriting={rewriteLoading}
          style="
            left: {eloPillHovered || eloPowerMenuOpen
            ? Math.max(8, baseWidgetRect.x - 68)
            : baseWidgetRect.x}px;
            top: {baseWidgetRect.y}px;
          "
          onmouseenter={() => {
            eloPillHovered = true;
            syncOverlayRegions();
          }}
          onmouseleave={() => {
            eloPillHovered = false;
            eloPillHoveredBtn = null;
            eloPowerMenuOpen = false;
            syncOverlayRegions();
          }}
          onpointerdown={onEloWidgetPointerDown}
          onpointermove={onEloWidgetPointerMove}
          onpointerup={onEloWidgetPointerUp}
        >
          <!-- Dark Tooltip Above Hovered Button (Image 4: "Rewrite with elo") -->
          {#if rewriteLoading && overlayPopoverMode !== "rewrite"}
            <div class="elo-dark-tooltip is-rewriting-tip">✨ Rewriting with elo…</div>
          {:else if eloPillHoveredBtn === "rewrite"}
            <div class="elo-dark-tooltip">
              {rewriteLoading ? "Rewriting with elo…" : "Rewrite with elo"}
            </div>
          {:else if eloPillHoveredBtn === "tone"}
            <div class="elo-dark-tooltip">
              Tone: {overlaySnap.tone || "Neutral"}
            </div>
          {:else if eloPillHoveredBtn === "power"}
            <div class="elo-dark-tooltip">Pause or disable elo</div>
          {:else if eloPillHoveredBtn === "count"}
            <div class="elo-dark-tooltip">
              {overlaySnap.alerts.length > 0
                ? `${overlaySnap.alerts.length} ${overlaySnap.alerts.length === 1 ? "suggestion" : "suggestions"} · Score ${overlayScore}`
                : `Score ${overlayScore} · No writing issues`}
            </div>
          {/if}

          <!-- Quick Power Menu when ⏻ is clicked -->
          {#if eloPowerMenuOpen}
            <div class="elo-power-menu">
              <button type="button" onclick={pauseEloForCurrentApp}>
                Pause elo in {overlaySnap.app_name || "this app"}
              </button>
              <button type="button" onclick={() => openStudioFromOverlay("apps", false)}>
                Manage app settings…
              </button>
            </div>
          {/if}

          <!-- Hover-Only Left Icons: Power (⏻) & Tone Megaphone (📢) (Image 2) -->
          {#if eloPillHovered || eloPowerMenuOpen}
            <button
              type="button"
              class="elo-sub-action-btn"
              aria-label="Pause or disable elo"
              onmouseenter={() => (eloPillHoveredBtn = "power")}
              onmouseleave={() => (eloPillHoveredBtn = null)}
              onclick={(e) => {
                e.stopPropagation();
                eloPowerMenuOpen = !eloPowerMenuOpen;
              }}
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none">
                <path
                  d="M12 3V12M18.36 6.64C19.99 8.27 21 10.52 21 13C21 17.97 16.97 22 12 22C7.03 22 3 17.97 3 13C3 10.52 4.01 8.27 5.64 6.64"
                  stroke="#525668"
                  stroke-width="2.1"
                  stroke-linecap="round"
                />
              </svg>
            </button>

            <button
              type="button"
              class="elo-sub-action-btn"
              aria-label="Detected tone"
              onmouseenter={() => (eloPillHoveredBtn = "tone")}
              onmouseleave={() => (eloPillHoveredBtn = null)}
              onclick={(e) => openRewritePopover(e)}
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none">
                <path
                  d="M3 11L19 4V20L3 13V11ZM19 9C20.66 9 22 10.34 22 12C22 13.66 20.66 15 19 15M6 14.5L7.5 20H10L8.8 15.5"
                  stroke="#525668"
                  stroke-width="1.9"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            </button>
          {/if}

          <!-- Always-Visible Sparkle Lightbulb Button ("Rewrite with elo", Images 1, 2, 4) -->
          <button
            type="button"
            class="elo-sub-action-btn bulb-btn"
            class:is-hovered={eloPillHoveredBtn === "rewrite"}
            class:is-busy={rewriteLoading}
            aria-label="Rewrite with elo"
            aria-busy={rewriteLoading}
            onmouseenter={() => (eloPillHoveredBtn = "rewrite")}
            onmouseleave={() => (eloPillHoveredBtn = null)}
            onclick={(e) => openRewritePopover(e)}
          >
            {#if rewriteLoading}
              <span class="elo-bulb-busy-wrap">
                <progress
                  aria-label="Rewriting text with elo"
                  class="loading-spinner elo-bulb-spinner"
                ></progress>
                {@render sparkleBulbIcon(16)}
              </span>
            {:else}
              {@render sparkleBulbIcon(20)}
            {/if}
          </button>

          <!-- Always-Visible Editorial Wink Mark + Count Badge (Brand Guide + Images 1 & 2) -->
          {#if overlaySnap.alerts.length > 0}
            <button
              type="button"
              class="elo-pill-wink-mark"
              aria-label="Open elo suggestions"
              onmouseenter={() => (eloPillHoveredBtn = "count")}
              onmouseleave={() => (eloPillHoveredBtn = null)}
              onclick={(e) => toggleFullEloCard(e)}
            >
              {@render editorialWinkLogo(19, "transparent", "#191F70")}
            </button>
          {/if}
          <button
            type="button"
            class="elo-count-badge-btn"
            class:style-only={overlaySnap.alerts.length > 0 &&
              (overlayCategoryCounts.correctness ?? 0) === 0}
            class:zero={overlaySnap.alerts.length === 0}
            aria-label="Review suggestions"
            onmouseenter={() => (eloPillHoveredBtn = "count")}
            onmouseleave={() => (eloPillHoveredBtn = null)}
            onclick={(e) => toggleFullEloCard(e)}
          >
            {#if overlaySnap.alerts.length > 0}
              <span>{overlaySnap.alerts.length}</span>
              {#if hasStyleOrClarityAlerts && (overlayCategoryCounts.correctness ?? 0) > 0}
                <span class="elo-badge-corner-dot"></span>
              {/if}
            {:else}
              {@render editorialWinkLogo(26, "#191F70", "#FFFFFF")}
            {/if}
          </button>
        </div>
      {/if}

      <!-- 3A. COMPACT INLINE QUICK-FIX MINI-POPOVER (Image 3) -->
      {#if overlayPopoverMode === "mini" && activeOverlayAlert && effectiveCardRect}
        <section
          class="elo-mini-popover"
          style="
            left: {effectiveCardRect.x}px;
            top: {effectiveCardRect.y}px;
            width: {effectiveCardRect.width}px;
          "
          aria-label="Quick suggestion"
        >
          <div class="mini-headline">
            {shortActionHeadline(
              activeOverlayAlert.subcategory,
              activeOverlayAlert.message,
              activeOverlayAlert.suggestions[0]?.kind,
            )}
          </div>

          {#if activeOverlayAlert.suggestions.length > 0}
            {#each activeOverlayAlert.suggestions.slice(0, 2) as sug, idx}
              <button
                type="button"
                class="mini-replacement-btn"
                onclick={() => applyOverlaySuggestion(activeOverlayAlert.id, idx)}
              >
                <span class="mini-repl-highlight">
                  {sug.kind === "remove" ? "Remove word" : sug.label}
                </span>
              </button>
            {/each}
          {/if}

          <button
            type="button"
            class="mini-action-row"
            onclick={() => dismissOverlayAlert(activeOverlayAlert.id)}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none">
              <path
                d="M4 7H20M10 11V17M14 11V17M5 7L6 19C6 20.1 6.9 21 8 21H16C17.1 21 18 20.1 18 19L19 7M9 7V4C9 3.45 9.45 3 10 3H14C14.55 3 15 3.45 15 4V7"
                stroke="#64687a"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span>Dismiss</span>
          </button>

          <button
            type="button"
            class="mini-action-row"
            onclick={() => {
              overlayPopoverMode = "full";
              syncOverlayRegions();
            }}
          >
            {@render editorialWinkLogo(18, "#191F70", "#FFFFFF")}
            <span>See more in elo</span>
          </button>
        </section>
      {/if}

      <!-- 3B. "WHAT DO YOU WANT TO DO?" GENERATIVE REWRITE POPOVER (Image 5) -->
      {#if overlayPopoverMode === "rewrite" && effectiveCardRect}
        <section
          class="elo-rewrite-card"
          style="
            left: {effectiveCardRect.x}px;
            top: {effectiveCardRect.y}px;
            width: {effectiveCardRect.width}px;
          "
          aria-label="Rewrite with elo"
        >
          <header class="rewrite-card-topbar">
            <div class="rewrite-brand-left">
              {@render editorialWinkLogo(24, "#191F70", "#FFFFFF")}
              <span class="elo-card-brand-name">elo</span>
            </div>
            <div class="rewrite-top-icons">
              <button
                type="button"
                class="elo-icon-btn active-spark"
                title="Rewrite ideas"
                onclick={() => (rewritePreview = null)}
              >
                ✨
              </button>
              <button
                type="button"
                class="elo-icon-btn"
                title="Voice & Tone Profile"
                onclick={() => openStudioFromOverlay("voice", false)}
              >
                ||||
              </button>
              <button
                type="button"
                class="elo-icon-btn"
                title="Close"
                onclick={() => {
                  overlayPopoverMode = "closed";
                  syncOverlayRegions();
                }}
              >
                ✕
              </button>
            </div>
          </header>

          {#if rewriteLoading}
            <div class="rewrite-ideas-body">
              <div class="rewrite-active-progress" role="status" aria-live="polite">
                <div class="rewrite-progress-row">
                  <progress
                    aria-label="Rewriting text with elo"
                    class="loading-spinner elo-card-spinner"
                  ></progress>
                  <div class="rewrite-progress-copy">
                    <strong>elo is rewriting your text…</strong>
                    <span>Refining clarity, tone, and flow</span>
                  </div>
                </div>
                <div class="rewrite-shimmer-lines" aria-hidden="true">
                  <span class="shimmer-bar w-92"></span>
                  <span class="shimmer-bar w-76"></span>
                </div>
              </div>
            </div>
          {:else if rewritePreview}
            <div class="rewrite-preview-body">
              <div class="rewrite-preview-head">
                <span class="rewrite-mode-pill">{rewritePreview.mode_label}</span>
                <span class="rewrite-engine-tag">{rewritePreview.engine}</span>
              </div>
              <div class="rewrite-preview-box">
                {rewritePreview.rewritten}
              </div>
              <div class="rewrite-preview-actions">
                <button type="button" class="elo-btn-accept" onclick={applyOverlayRewriteToApp}>
                  Insert in {overlaySnap.app_name || "App"}
                </button>
                <button
                  type="button"
                  class="elo-btn-alt"
                  onclick={() => navigator.clipboard?.writeText(rewritePreview?.rewritten ?? "")}
                >
                  Copy
                </button>
                <button
                  type="button"
                  class="elo-btn-dismiss"
                  onclick={() => (rewritePreview = null)}
                >
                  ← Ideas
                </button>
              </div>
            </div>
          {:else}
            <div class="rewrite-ideas-body">
              <h3>What do you want to do?</h3>
              <p class="rewrite-sub">Here are some ideas</p>

              <div class="rewrite-idea-list">
                <button
                  type="button"
                  class="rewrite-idea-row"
                  disabled={rewriteLoading}
                  onclick={() => triggerOverlayRewrite("improve")}
                >
                  <span class="idea-emoji">✏️</span>
                  <span>Improve it</span>
                </button>

                <button
                  type="button"
                  class="rewrite-idea-row"
                  disabled={rewriteLoading}
                  onclick={() => triggerOverlayRewrite("confident")}
                >
                  <span class="idea-emoji">🤝</span>
                  <span>Sound confident</span>
                </button>

                <div class="rewrite-more-row">
                  <button
                    type="button"
                    class="rewrite-idea-row flex-1"
                    disabled={rewriteLoading}
                    onclick={() => {
                      rewriteMoreIdeasOpen = !rewriteMoreIdeasOpen;
                      syncOverlayRegions();
                    }}
                  >
                    <span class="idea-emoji">⊞</span>
                    <span>More ideas</span>
                  </button>
                  {#if rewriteHistory.length > 0}
                    <button
                      type="button"
                      class="elo-icon-btn"
                      title="Restore last rewrite"
                      disabled={rewriteLoading}
                      onclick={() => (rewritePreview = rewriteHistory[0])}
                    >
                      ↺
                    </button>
                  {/if}
                </div>

                {#if rewriteMoreIdeasOpen}
                  <div class="rewrite-extra-ideas">
                    <button
                      type="button"
                      class="rewrite-chip"
                      disabled={rewriteLoading}
                      onclick={() => triggerOverlayRewrite("concise")}
                    >
                      🎯 Make it concise
                    </button>
                    <button
                      type="button"
                      class="rewrite-chip"
                      disabled={rewriteLoading}
                      onclick={() => triggerOverlayRewrite("formal")}
                    >
                      🎩 Sound formal
                    </button>
                    <button
                      type="button"
                      class="rewrite-chip"
                      disabled={rewriteLoading}
                      onclick={() => triggerOverlayRewrite("friendly")}
                    >
                      😊 Sound friendly
                    </button>
                    <button
                      type="button"
                      class="rewrite-chip"
                      disabled={rewriteLoading}
                      onclick={() => triggerOverlayRewrite("bullets")}
                    >
                      • Bullet points
                    </button>
                  </div>
                {/if}
              </div>

              {#if rewriteError}
                <p class="rewrite-err">{rewriteError}</p>
              {/if}

              <!-- "Tell us to..." / "Tell elo to..." Custom Prompt Input (Image 5) -->
              <form
                class="rewrite-prompt-form"
                onpointerdown={(e) => {
                  const target = e.target as HTMLElement | null;
                  if (!target?.closest(".rewrite-prompt-send")) {
                    focusOverlayPromptInput();
                  }
                }}
                onsubmit={(e) => {
                  e.preventDefault();
                  if (!rewriteLoading && rewriteCustomPrompt.trim()) {
                    const prompt = rewriteCustomPrompt.trim();
                    rewriteCustomPrompt = "";
                    triggerOverlayRewrite("custom", prompt);
                  }
                }}
              >
                <input
                  type="text"
                  class="rewrite-prompt-input"
                  bind:this={overlayPromptInputEl}
                  placeholder={rewriteLoading ? "Rewriting with elo…" : "Tell elo to..."}
                  bind:value={rewriteCustomPrompt}
                  disabled={rewriteLoading}
                  onclick={() => focusOverlayPromptInput()}
                  onfocus={() => {
                    if (!overlayInputFocused) {
                      focusOverlayPromptInput();
                    }
                  }}
                  onkeydown={(e) => {
                    if (e.key === "Escape") {
                      e.preventDefault();
                      releaseOverlayPromptInput();
                      overlayPopoverMode = "closed";
                      syncOverlayRegions();
                    }
                  }}
                />
                {#if rewriteCustomPrompt.trim() && !rewriteLoading}
                  <button type="submit" class="rewrite-prompt-send">→</button>
                {/if}
              </form>
            </div>
          {/if}
        </section>
      {/if}

      <!-- 3C. FULL MULTI-ALERT SUGGESTION CARD -->
      {#if overlayPopoverMode === "full" && effectiveCardRect}
        <section
          class="elo-floating-card"
          style="
            left: {effectiveCardRect.x}px;
            top: {effectiveCardRect.y}px;
            width: {effectiveCardRect.width}px;
          "
          aria-label="elo writing assistant"
        >
          <header class="elo-card-topbar">
            <div class="elo-card-brand">
              {@render editorialWinkLogo(22, "#191F70", "#FFFFFF")}
              <span class="elo-card-brand-name">elo</span>
              {#if overlaySnap.app_name}
                <span class="elo-card-app-pill">{overlaySnap.app_name}</span>
              {/if}
              <span
                class="elo-card-score-pill"
                style="--score-accent: {scoreColorHex(overlayScore)};"
                title="Overall writing score"
              >
                Score {overlayScore}
              </span>
            </div>

            <div class="elo-card-top-actions">
              {#if overlaySnap.alerts.length > 1}
                <div class="elo-card-stepper">
                  <button
                    type="button"
                    class="elo-icon-btn"
                    title="Previous suggestion"
                    onclick={() => handleOverlayCardCommand("prev")}
                  >
                    ‹
                  </button>
                  <span class="elo-step-counter">
                    {activeOverlayAlertIndex + 1} / {overlaySnap.alerts.length}
                  </span>
                  <button
                    type="button"
                    class="elo-icon-btn"
                    title="Next suggestion"
                    onclick={() => handleOverlayCardCommand("next")}
                  >
                    ›
                  </button>
                </div>
              {/if}

              <button
                type="button"
                class="elo-icon-btn"
                title="Rewrite with elo"
                onclick={(e) => openRewritePopover(e)}
              >
                ✨
              </button>
              <button
                type="button"
                class="elo-icon-btn"
                title="Open in Elocutio Full Editor"
                onclick={() => openStudioFromOverlay("editor", true)}
              >
                ⤢
              </button>
              <button
                type="button"
                class="elo-icon-btn"
                title="Elocutio Settings"
                onclick={() => openStudioFromOverlay("apps", false)}
              >
                ⚙
              </button>
              <button
                type="button"
                class="elo-icon-btn"
                title="Close card"
                onclick={() => {
                  overlayPopoverMode = "closed";
                  syncOverlayRegions();
                }}
              >
                ✕
              </button>
            </div>
          </header>

          {#if activeOverlayAlert}
            <div class="elo-active-alert-box">
              <div class="elo-alert-cat-row">
                <span class="elo-cat-dot" style="background: {activeOverlayAlert.color};"></span>
                <span class="elo-cat-title">
                  <strong>{activeOverlayAlert.category_label}:</strong>
                  {activeOverlayAlert.subcategory}
                </span>
              </div>

              <div class="elo-context-sentence">
                <span class="ctx-muted">{activeOverlayAlert.context_before}</span>
                {#if activeOverlayAlert.suggestions.length > 0}
                  <del class="ctx-del">{activeOverlayAlert.original}</del>
                  <ins class="ctx-ins">
                    {activeOverlayAlert.suggestions[0].replacement || "∅"}
                  </ins>
                {:else}
                  <mark class="ctx-highlight">{activeOverlayAlert.original}</mark>
                {/if}
                <span class="ctx-muted">{activeOverlayAlert.context_after}</span>
              </div>

              <p class="elo-alert-explanation">{activeOverlayAlert.message}</p>

              <div class="elo-alert-actions">
                <div class="elo-primary-actions">
                  {#if activeOverlayAlert.suggestions.length > 0}
                    {#each activeOverlayAlert.suggestions.slice(0, 3) as sug, idx}
                      <button
                        type="button"
                        class={idx === 0 ? "elo-btn-accept" : "elo-btn-alt"}
                        onclick={() => applyOverlaySuggestion(activeOverlayAlert.id, idx)}
                      >
                        {idx === 0
                          ? sug.kind === "remove"
                            ? "Accept (Remove)"
                            : `Accept "${sug.label}"`
                          : sug.label}
                      </button>
                    {/each}
                  {:else}
                    <button
                      type="button"
                      class="elo-btn-accept"
                      onclick={() => dismissOverlayAlert(activeOverlayAlert.id)}
                    >
                      Got it
                    </button>
                  {/if}

                  <button
                    type="button"
                    class="elo-btn-dismiss"
                    onclick={() => dismissOverlayAlert(activeOverlayAlert.id)}
                  >
                    Dismiss
                  </button>
                </div>

                <div class="elo-more-wrap">
                  <button
                    type="button"
                    class="elo-icon-btn"
                    title="More actions"
                    onclick={() => (overlayMoreMenuOpen = !overlayMoreMenuOpen)}
                  >
                    •••
                  </button>
                  {#if overlayMoreMenuOpen}
                    <div class="elo-more-dropdown">
                      {#if canAddToLexicon(activeOverlayAlert.original, activeOverlayAlert.subcategory)}
                        <button
                          type="button"
                          onclick={() =>
                            addOverlayWordToDictionary(
                              activeOverlayAlert.original,
                              activeOverlayAlert.id,
                            )}
                        >
                          Add "{activeOverlayAlert.original}" to dictionary
                        </button>
                      {/if}
                      <button
                        type="button"
                        onclick={() =>
                          disableOverlayRule(activeOverlayAlert.rule_name, activeOverlayAlert.id)}
                      >
                        Turn off rule ({activeOverlayAlert.subcategory})
                      </button>
                    </div>
                  {/if}
                </div>
              </div>
            </div>

            {#if overlaySnap.alerts.length > 1}
              <div class="elo-compact-alert-list scroller">
                {#each overlaySnap.alerts as item (item.id)}
                  {#if item.id !== activeOverlayAlert.id}
                    <button
                      type="button"
                      class="elo-compact-alert-row"
                      onclick={() => {
                        overlayActiveAlertId = item.id;
                        overlayMoreMenuOpen = false;
                      }}
                    >
                      <span class="elo-cat-dot" style="background: {item.color};"></span>
                      <span class="compact-orig">{item.original}</span>
                      <span class="compact-sep">·</span>
                      <span class="compact-sub">
                        {item.suggestions[0]?.label
                          ? `→ ${item.suggestions[0].label}`
                          : item.subcategory}
                      </span>
                    </button>
                  {/if}
                {/each}
              </div>
            {/if}
          {:else}
            <div class="elo-clean-state">
              <div class="elo-clean-badge">✓</div>
              <div>
                <strong>Clear, thoughtful, and accurate.</strong>
                <p>
                  No issues found across {overlaySnap.word_count} words{overlaySnap.tone
                    ? ` · Tone: ${overlaySnap.tone}`
                    : ""}.
                </p>
              </div>
            </div>
          {/if}

          <footer class="elo-card-cat-footer">
            {#each [{ id: "correctness", label: "Correctness", color: "#E5484D" }, { id: "clarity", label: "Clarity", color: "#2563EB" }, { id: "engagement", label: "Engagement", color: "#10B981" }, { id: "delivery", label: "Delivery", color: "#7C3AED" }, { id: "style", label: "Style guide", color: "#191F70" }] as cat}
              {@const count = overlayCategoryCounts[cat.id] ?? 0}
              {@const catScore = overlayCategoryScores[cat.id] ?? 100}
              <button
                type="button"
                class="elo-cat-footer-item"
                class:is-selected={overlayCategoryFilter === cat.id}
                title="{cat.label}: {catScore}% ({count} {count === 1 ? 'alert' : 'alerts'})"
                onclick={() =>
                  (overlayCategoryFilter = overlayCategoryFilter === cat.id ? "all" : cat.id)}
              >
                <span class="elo-cat-track">
                  <span class="elo-cat-bar" style="width: {catScore}%; background: {cat.color};"
                  ></span>
                </span>
                <span class="elo-cat-row">
                  <span class="elo-cat-name">{cat.label}</span>
                  {#if count > 0}
                    <span class="elo-cat-count" style="color: {cat.color};">{count}</span>
                  {/if}
                </span>
              </button>
            {/each}
          </footer>
        </section>
      {/if}
    {/if}
  </div>
{:else}
  <!-- =======================================================================
       MODE B: MAIN DESKTOP APPLICATION (ELOCUTIO EDITORIAL ATELIER)
       ======================================================================= -->
  <div class="studio-shell atelier-shell">
    <!-- LEFT MIDNIGHT INDIGO ATELIER RAIL -->
    <aside class="atelier-rail" aria-label="Elocutio Studio Navigation">
      <div class="atelier-rail-top">
        <button
          type="button"
          class="atelier-monogram-btn"
          title="Elocutio — Literary Writing Studio"
          onclick={() => (activeTab = "editor")}
        >
          {@render editorialWinkLogo(34, "#FFFFFF", "#191F70")}
          <span class="atelier-monogram-word">elo</span>
        </button>

        <div class="atelier-rail-divider" aria-hidden="true"></div>

        <nav class="atelier-rail-nav" aria-label="Main sections">
          <button
            type="button"
            class="atelier-rail-btn"
            class:active={activeTab === "editor"}
            onclick={() => (activeTab = "editor")}
          >
            <svg
              class="rail-svg"
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <path
                d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span class="rail-label">Manuscript</span>
            {#if editorLints.length > 0}
              <span class="rail-badge">{editorLints.length}</span>
            {/if}
          </button>

          <button
            type="button"
            class="atelier-rail-btn"
            class:active={activeTab === "analytics"}
            onclick={() => {
              activeTab = "analytics";
              loadAllSettings();
            }}
          >
            <svg
              class="rail-svg"
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <path
                d="M4 19.5h16M7 16v-5M12 16V7M17 16v-8"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span class="rail-label">Analytics</span>
          </button>

          <button
            type="button"
            class="atelier-rail-btn"
            class:active={activeTab === "apps"}
            onclick={() => {
              activeTab = "apps";
              loadAllSettings();
            }}
          >
            <svg
              class="rail-svg"
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <rect
                x="3"
                y="3"
                width="7"
                height="7"
                rx="1.5"
                stroke="currentColor"
                stroke-width="1.8"
              />
              <rect
                x="14"
                y="3"
                width="7"
                height="7"
                rx="1.5"
                stroke="currentColor"
                stroke-width="1.8"
              />
              <rect
                x="3"
                y="14"
                width="7"
                height="7"
                rx="1.5"
                stroke="currentColor"
                stroke-width="1.8"
              />
              <rect
                x="14"
                y="14"
                width="7"
                height="7"
                rx="1.5"
                stroke="currentColor"
                stroke-width="1.8"
              />
            </svg>
            <span class="rail-label">Apps</span>
            {#if axStatus !== "Granted"}
              <span class="rail-badge warn">!</span>
            {/if}
          </button>

          <button
            type="button"
            class="atelier-rail-btn"
            class:active={activeTab === "rules"}
            onclick={() => {
              activeTab = "rules";
              loadAllSettings();
            }}
          >
            <svg
              class="rail-svg"
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <path
                d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span class="rail-label">Lexicon</span>
          </button>

          <button
            type="button"
            class="atelier-rail-btn"
            class:active={activeTab === "voice"}
            onclick={() => {
              activeTab = "voice";
              loadAllSettings();
            }}
          >
            <svg
              class="rail-svg"
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <path
                d="M2 12h2m4-6v12m4-9v6m4-10v14m4-7h2"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span class="rail-label">Voice DNA</span>
          </button>

          <button
            type="button"
            class="atelier-rail-btn"
            class:active={activeTab === "ai"}
            onclick={() => {
              activeTab = "ai";
              loadAllSettings();
            }}
          >
            <svg
              class="rail-svg"
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <path
                d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span class="rail-label">Privacy & AI</span>
          </button>
        </nav>
      </div>

      <div class="atelier-rail-bottom">
        <button
          type="button"
          class="atelier-rail-btn rail-companion-toggle"
          title={serviceRunning
            ? 'System "elo" widget is active (click to pause)'
            : 'System "elo" widget is paused (click to enable)'}
          onclick={toggleEloService}
        >
          <span class="rail-companion-icon-wrap">
            <svg
              class="rail-svg"
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <path
                d="M12 2v4m0 12v4M4.93 4.93l2.83 2.83m8.48 8.48 2.83 2.83M2 12h4m12 0h4M4.93 19.07l2.83-2.83m8.48-8.48 2.83-2.83"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
              />
            </svg>
            <span class="rail-status-led" class:is-on={serviceRunning}></span>
          </span>
          <span class="rail-label">{serviceRunning ? "Overlay On" : "Paused"}</span>
        </button>
      </div>
    </aside>

    <!-- RIGHT WORKSPACE DESK -->
    <div class="atelier-desk">
      {#if activeTab === "editor"}
        <main class="atelier-editor-grid">
          <!-- CENTER PANE: RAISED WARM IVORY MANUSCRIPT FOLIO -->
          <section class="manuscript-folio">
            <header class="folio-header">
              <div class="folio-top-bar">
                <div class="folio-eyebrow">
                  <span>MANUSCRIPT FOLIO</span>
                  <span class="eyebrow-dot">·</span>
                  <span>VOICE: {editorTone.toUpperCase()}</span>
                  <span class="eyebrow-dot">·</span>
                  <span>{dialectName} STANDARD</span>
                </div>
                <div class="doc-sample-actions">
                  <button
                    type="button"
                    class="doc-chip-btn"
                    onclick={() => {
                      docTitle = "Welcome to the Elocutio Editor";
                      editorText = SAMPLE_WELCOME;
                      runEditorCheck();
                    }}
                  >
                    Sample 1
                  </button>
                  <button
                    type="button"
                    class="doc-chip-btn"
                    onclick={() => {
                      docTitle = "Budget-friendly transformations";
                      editorText = SAMPLE_BUDGET;
                      runEditorCheck();
                    }}
                  >
                    Sample 2
                  </button>
                  <button
                    type="button"
                    class="doc-chip-btn"
                    onclick={() => {
                      docTitle = "Untitled Draft";
                      editorText = "";
                      runEditorCheck();
                    }}
                  >
                    Clear
                  </button>
                </div>
              </div>
              <input
                type="text"
                class="doc-title-input"
                bind:value={docTitle}
                placeholder="Untitled manuscript"
              />
            </header>

            <div class="canvas-stage">
              <div class="canvas-backdrop scroller" bind:this={backdropEl} aria-hidden="true">
                {#each highlightedSegments as seg}
                  {#if seg.lint}
                    <mark
                      class="inline-lint-mark"
                      class:selected={activeEditorLint?.id === seg.lint.id}
                      style="--lint-color: {seg.lint.color};">{seg.text}</mark
                    >
                  {:else}
                    <span>{seg.text}</span>
                  {/if}
                {/each}
                {#if editorText.endsWith("\n")}
                  <br />
                {/if}
              </div>

              <textarea
                class="canvas-textarea scroller"
                bind:this={textareaEl}
                bind:value={editorText}
                oninput={scheduleEditorCheck}
                onscroll={syncScroll}
                onclick={handleTextareaClick}
                spellcheck="false"
                placeholder="Start writing or paste your text here..."></textarea>

              <!-- Inline Quick-Fix Mini-Popover OR "What do you want to do?" Card inside Studio Canvas -->
              {#if editorInlineCardPos && editorInlineCardMode === "mini" && activeEditorLint}
                <div
                  class="elo-mini-popover studio-pos"
                  style="
                  left: {editorInlineCardPos.x}px;
                  top: {editorInlineCardPos.y}px;
                "
                >
                  <div class="mini-headline">
                    {shortActionHeadline(
                      activeEditorLint.subcategory,
                      activeEditorLint.message,
                      activeEditorLint.suggestions[0]?.kind,
                    )}
                  </div>

                  {#if activeEditorLint.suggestions.length > 0}
                    {#each activeEditorLint.suggestions.slice(0, 2) as sug}
                      <button
                        type="button"
                        class="mini-replacement-btn"
                        onclick={() => applyEditorSuggestion(activeEditorLint, sug.text)}
                      >
                        <span class="mini-repl-highlight">
                          {sug.kind === "remove" ? "Remove word" : sug.label}
                        </span>
                      </button>
                    {/each}
                  {/if}

                  <button
                    type="button"
                    class="mini-action-row"
                    onclick={() => dismissEditorLint(activeEditorLint.id)}
                  >
                    <span>🗑</span>
                    <span>Dismiss</span>
                  </button>

                  <button
                    type="button"
                    class="mini-action-row"
                    onclick={() => (editorInlineCardPos = null)}
                  >
                    {@render editorialWinkLogo(18, "#191F70", "#FFFFFF")}
                    <span>See details in All alerts →</span>
                  </button>
                </div>
              {:else if editorInlineCardPos && editorInlineCardMode === "rewrite"}
                <div
                  class="elo-rewrite-card studio-pos"
                  style="right: 28px; bottom: 64px; width: 356px;"
                >
                  <header class="rewrite-card-topbar">
                    <div class="rewrite-brand-left">
                      {@render editorialWinkLogo(22, "#191F70", "#FFFFFF")}
                      <span class="elo-card-brand-name">elo</span>
                    </div>
                    <div class="rewrite-top-icons">
                      <button
                        type="button"
                        class="elo-icon-btn"
                        onclick={() => (editorInlineCardPos = null)}
                      >
                        ✕
                      </button>
                    </div>
                  </header>

                  <div class="rewrite-ideas-body">
                    {#if aiRewriteLoading}
                      <div class="rewrite-active-progress" role="status" aria-live="polite">
                        <div class="rewrite-progress-row">
                          <progress
                            aria-label="Rewriting text with elo"
                            class="loading-spinner elo-card-spinner"
                          ></progress>
                          <div class="rewrite-progress-copy">
                            <strong>elo is rewriting your text…</strong>
                            <span>Refining clarity, tone, and flow</span>
                          </div>
                        </div>
                        <div class="rewrite-shimmer-lines" aria-hidden="true">
                          <span class="shimmer-bar w-92"></span>
                          <span class="shimmer-bar w-76"></span>
                        </div>
                      </div>
                    {:else}
                      <h3>What do you want to do?</h3>
                      <p class="rewrite-sub">Here are some ideas</p>

                      <div class="rewrite-idea-list">
                        <button
                          type="button"
                          class="rewrite-idea-row"
                          disabled={aiRewriteLoading}
                          onclick={() => runStudioIdeaRewrite("improve")}
                        >
                          <span class="idea-emoji">✏️</span>
                          <span>Improve it</span>
                        </button>
                        <button
                          type="button"
                          class="rewrite-idea-row"
                          disabled={aiRewriteLoading}
                          onclick={() => runStudioIdeaRewrite("confident")}
                        >
                          <span class="idea-emoji">🤝</span>
                          <span>Sound confident</span>
                        </button>
                        <div class="rewrite-more-row">
                          <button
                            type="button"
                            class="rewrite-idea-row flex-1"
                            disabled={aiRewriteLoading}
                            onclick={() => (rewriteMoreIdeasOpen = !rewriteMoreIdeasOpen)}
                          >
                            <span class="idea-emoji">⊞</span>
                            <span>More ideas</span>
                          </button>
                        </div>
                        {#if rewriteMoreIdeasOpen}
                          <div class="rewrite-extra-ideas">
                            <button
                              type="button"
                              class="rewrite-chip"
                              disabled={aiRewriteLoading}
                              onclick={() => runStudioIdeaRewrite("concise")}
                            >
                              🎯 Make it concise
                            </button>
                            <button
                              type="button"
                              class="rewrite-chip"
                              disabled={aiRewriteLoading}
                              onclick={() => runStudioIdeaRewrite("formal")}
                            >
                              🎩 Sound formal
                            </button>
                            <button
                              type="button"
                              class="rewrite-chip"
                              disabled={aiRewriteLoading}
                              onclick={() => runStudioIdeaRewrite("friendly")}
                            >
                              😊 Sound friendly
                            </button>
                            <button
                              type="button"
                              class="rewrite-chip"
                              disabled={aiRewriteLoading}
                              onclick={() => runStudioIdeaRewrite("bullets")}
                            >
                              • Bullet points
                            </button>
                          </div>
                        {/if}
                      </div>

                      {#if aiRewriteResult}
                        <div class="rewrite-preview-box">
                          {aiRewriteResult}
                        </div>
                        <div class="rewrite-preview-actions">
                          <button
                            type="button"
                            class="elo-btn-accept sm"
                            onclick={() => {
                              if (aiRewriteResult) {
                                editorText = aiRewriteResult;
                                aiRewriteResult = null;
                                editorInlineCardPos = null;
                                runEditorCheck();
                              }
                            }}
                          >
                            Replace in Editor
                          </button>
                          <button
                            type="button"
                            class="elo-btn-dismiss sm"
                            onclick={() => (aiRewriteResult = null)}
                          >
                            Discard
                          </button>
                        </div>
                      {/if}

                      {#if aiRewriteError}
                        <p class="rewrite-err">{aiRewriteError}</p>
                      {/if}

                      <form
                        class="rewrite-prompt-form"
                        onsubmit={(e) => {
                          e.preventDefault();
                          if (!aiRewriteLoading && studioCustomPrompt.trim()) {
                            const prompt = studioCustomPrompt.trim();
                            studioCustomPrompt = "";
                            runStudioIdeaRewrite("custom", prompt);
                          }
                        }}
                      >
                        <input
                          type="text"
                          class="rewrite-prompt-input"
                          placeholder="Tell elo to..."
                          bind:value={studioCustomPrompt}
                          disabled={aiRewriteLoading}
                        />
                        {#if studioCustomPrompt.trim() && !aiRewriteLoading}
                          <button type="submit" class="rewrite-prompt-send">→</button>
                        {/if}
                      </form>
                    {/if}
                  </div>
                </div>
              {/if}

              <!-- Draggable & Hover-Expandable Inline "elo" Teardrop Widget inside Studio Canvas -->
              <div
                role="group"
                aria-label="Studio elo widget"
                class="elo-teardrop-widget studio-inline-pos"
                class:is-expanded={studioEloHovered}
                class:is-rewriting={aiRewriteLoading}
                style="transform: translate({studioEloOffset.x}px, {studioEloOffset.y}px);"
                onmouseenter={() => (studioEloHovered = true)}
                onmouseleave={() => {
                  studioEloHovered = false;
                  studioEloHoveredBtn = null;
                }}
                onpointerdown={onStudioEloPointerDown}
                onpointermove={onStudioEloPointerMove}
                onpointerup={onStudioEloPointerUp}
              >
                {#if aiRewriteLoading}
                  <div class="elo-dark-tooltip is-rewriting-tip">✨ Rewriting with elo…</div>
                {:else if studioEloHoveredBtn === "rewrite"}
                  <div class="elo-dark-tooltip">Rewrite with elo</div>
                {:else if studioEloHoveredBtn === "tone"}
                  <div class="elo-dark-tooltip">Tone: {editorTone}</div>
                {/if}

                {#if studioEloHovered}
                  <button
                    type="button"
                    class="elo-sub-action-btn"
                    aria-label="Detected tone"
                    onmouseenter={() => (studioEloHoveredBtn = "tone")}
                    onmouseleave={() => (studioEloHoveredBtn = null)}
                    onclick={(e) => openStudioRewriteCard(e)}
                  >
                    📢
                  </button>
                {/if}

                <button
                  type="button"
                  class="elo-sub-action-btn bulb-btn"
                  class:is-busy={aiRewriteLoading}
                  aria-label="Rewrite with elo"
                  aria-busy={aiRewriteLoading}
                  onmouseenter={() => (studioEloHoveredBtn = "rewrite")}
                  onmouseleave={() => (studioEloHoveredBtn = null)}
                  onclick={(e) => openStudioRewriteCard(e)}
                >
                  {#if aiRewriteLoading}
                    <span class="elo-bulb-busy-wrap">
                      <progress
                        aria-label="Rewriting text with elo"
                        class="loading-spinner elo-bulb-spinner"
                      ></progress>
                      {@render sparkleBulbIcon(16)}
                    </span>
                  {:else}
                    {@render sparkleBulbIcon(20)}
                  {/if}
                </button>

                {#if editorLints.length > 0}
                  <span class="elo-pill-wink-mark" aria-hidden="true">
                    {@render editorialWinkLogo(19, "transparent", "#191F70")}
                  </span>
                {/if}

                <button
                  type="button"
                  class="elo-count-badge-btn"
                  class:zero={editorLints.length === 0}
                  aria-label="Cycle alerts"
                >
                  {#if editorLints.length > 0}
                    <span>{editorLints.length}</span>
                    <span class="elo-badge-corner-dot"></span>
                  {:else}
                    {@render editorialWinkLogo(26, "#191F70", "#FFFFFF")}
                  {/if}
                </button>
              </div>
            </div>

            <footer class="folio-colophon">
              <div class="format-tools">
                <button type="button" title="Bold" onclick={() => insertMarkdownWrap("**", "**")}>
                  <strong>B</strong>
                </button>
                <button type="button" title="Italic" onclick={() => insertMarkdownWrap("*", "*")}>
                  <em>I</em>
                </button>
                <button
                  type="button"
                  title="Heading 1"
                  onclick={() => insertMarkdownWrap("# ", "")}
                >
                  H1
                </button>
                <button
                  type="button"
                  title="Heading 2"
                  onclick={() => insertMarkdownWrap("## ", "")}
                >
                  H2
                </button>
                <button
                  type="button"
                  title="Bullet List"
                  onclick={() => insertMarkdownWrap("\n- ", "")}
                >
                  • List
                </button>
              </div>

              <div class="canvas-metrics">
                <span class="tone-chip">Tone: {editorTone}</span>
                <span>
                  {editorWordCount} words · {editorCharCount} chars · ~{Math.max(
                    1,
                    Math.ceil(editorWordCount / 220),
                  )} min read
                </span>
              </div>
            </footer>
          </section>

          <!-- RIGHT PANE: UNIFIED "PROOFREADER'S MARGIN" (Replaces Grammarly 3-Col Split) -->
          <aside class="proofreader-margin">
            <!-- 1. EDITORIAL INDEX CARD (Replaces Circular Score Ring & Progress Bars) -->
            <div class="editorial-index-card">
              <div class="index-card-top">
                <div>
                  <div class="index-eyebrow">EDITORIAL INDEX</div>
                  <div class="index-score-row">
                    <span class="index-score-num" style="color: {scoreColorHex(editorScore)};">
                      {editorScore}
                    </span>
                    <span
                      class="index-status-pill"
                      style="--pill-accent: {scoreColorHex(editorScore)};"
                    >
                      {editorLints.length === 0
                        ? "Polished Draft"
                        : editorScore >= 85
                          ? "Crisp Prose"
                          : "Needs Proofing"}
                    </span>
                  </div>
                </div>

                <button
                  type="button"
                  class="goals-btn"
                  onclick={() => (studioGoalsOpen = !studioGoalsOpen)}
                >
                  <span>✦</span>
                  <span>Calibrate Goals</span>
                </button>
              </div>

              {#if studioGoalsOpen}
                <div class="goals-drawer">
                  <h4>Editorial Standard & Target Register</h4>
                  <label class="goal-field">
                    <span>English Dialect</span>
                    <select
                      value={dialectName}
                      onchange={(e) => changeDialect((e.currentTarget as HTMLSelectElement).value)}
                    >
                      <option value="US">American (US)</option>
                      <option value="GB">British (UK)</option>
                      <option value="CA">Canadian (CA)</option>
                      <option value="AU">Australian (AU)</option>
                      <option value="IN">Indian (IN)</option>
                    </select>
                  </label>
                  <label class="goal-field">
                    <span>Target Register</span>
                    <select bind:value={selectedRewriteTone}>
                      <option value="improve">Editorial Polish</option>
                      <option value="concise">Concise & Direct</option>
                      <option value="formal">Executive & Formal</option>
                      <option value="friendly">Warm & Conversational</option>
                    </select>
                  </label>
                </div>
              {/if}

              <!-- 5 Interactive Dimension Tiles (Click to Filter Margin Notes) -->
              <div class="dimension-grid">
                {#each [{ id: "correctness", short: "Proof", label: "Correctness", color: "#C84B31" }, { id: "clarity", short: "Clarity", label: "Clarity", color: "#191F70" }, { id: "engagement", short: "Verve", label: "Engagement", color: "#1E6F5C" }, { id: "delivery", short: "Tone", label: "Delivery", color: "#6D28D9" }, { id: "style", short: "Style", label: "Style guide", color: "#191F70" }] as cat}
                  {@const count = editorCategoryCounts[cat.id] ?? 0}
                  {@const catScore = editorCategoryScores[cat.id] ?? 100}
                  <button
                    type="button"
                    class="dimension-tile"
                    class:active={editorCategoryFilter === cat.id}
                    title="{cat.label}: {catScore}% ({count} {count === 1 ? 'note' : 'notes'})"
                    onclick={() =>
                      (editorCategoryFilter = editorCategoryFilter === cat.id ? "all" : cat.id)}
                  >
                    <span class="dim-score" style="color: {count > 0 ? cat.color : '#191F70'};">
                      {catScore}
                    </span>
                    <span class="dim-name">{cat.short}</span>
                    {#if count > 0}
                      <span class="dim-count-dot" style="background: {cat.color};">
                        {count}
                      </span>
                    {/if}
                  </button>
                {/each}
              </div>
            </div>

            <!-- 2. MARGIN ANNOTATIONS HEADER & FILTER BAR -->
            <div class="margin-section-bar">
              <div class="margin-section-left">
                <button
                  type="button"
                  class="margin-filter-reset"
                  class:is-filtered={editorCategoryFilter !== "all"}
                  onclick={() => (editorCategoryFilter = "all")}
                >
                  <span>
                    {editorCategoryFilter === "all"
                      ? "MARGIN ANNOTATIONS"
                      : `${editorCategoryFilter.toUpperCase()} NOTES`}
                  </span>
                  <span class="margin-count-pill">{filteredEditorLints.length}</span>
                </button>
                {#if editorCategoryFilter !== "all"}
                  <button
                    type="button"
                    class="show-all-link"
                    onclick={() => (editorCategoryFilter = "all")}
                  >
                    Show all ({editorLints.length})
                  </button>
                {/if}
              </div>

              {#if filteredEditorLints.length > 1}
                <button type="button" class="accept-all-btn" onclick={acceptAllEditorSuggestions}>
                  Apply all ({filteredEditorLints.length}) →
                </button>
              {/if}
            </div>

            <!-- 3. LITERARY MARGIN NOTES FEED -->
            <div class="margin-notes-feed scroller">
              {#if filteredEditorLints.length === 0}
                <div class="alerts-empty-card">
                  {@render editorialWinkLogo(36, "#191F70", "#FFFFFF")}
                  <h3>Impeccable Manuscript</h3>
                  <p>
                    No editorial notes in this view. Continue drafting on the manuscript sheet or
                    use Rewrite Atelier below.
                  </p>
                </div>
              {:else}
                {#each filteredEditorLints as lint, idx (lint.id)}
                  {@const isExpanded = activeEditorLint?.id === lint.id}
                  {#if isExpanded}
                    <article class="margin-note-card expanded" style="--card-accent: {lint.color};">
                      <div class="note-card-top">
                        <span class="note-eyebrow" style="color: {lint.color};">
                          {lint.category_label.toUpperCase()} · {lint.subcategory}
                        </span>
                        <span class="note-index-tag">Note #{idx + 1}</span>
                      </div>

                      <div class="note-diff-box">
                        {#if lint.suggestions.length > 0}
                          <del class="feed-orig">{lint.original}</del>
                          <span class="feed-arrow">→</span>
                          {#each lint.suggestions.slice(0, 3) as sug}
                            <button
                              type="button"
                              class="literary-replacement-btn"
                              onclick={() => applyEditorSuggestion(lint, sug.text)}
                            >
                              {sug.kind === "remove" ? "Omit word" : sug.label}
                            </button>
                          {/each}
                        {:else}
                          <span class="feed-excerpt">{lint.original}</span>
                        {/if}
                      </div>

                      <p class="feed-card-explanation">{lint.message}</p>

                      <div class="feed-card-footer">
                        <div class="feed-card-btns">
                          {#if lint.suggestions[0]}
                            <button
                              type="button"
                              class="elo-btn-accept"
                              onclick={() => applyEditorSuggestion(lint, lint.suggestions[0].text)}
                            >
                              Apply Edit
                            </button>
                          {:else if lint.original.trim().split(/\s+/).length >= 8}
                            <button
                              type="button"
                              class="elo-btn-accept"
                              disabled={rewritingLintId === lint.id}
                              onclick={() => rewriteSingleSentenceLint(lint)}
                            >
                              {rewritingLintId === lint.id ? "Tightening…" : "✦ Tighten Sentence"}
                            </button>
                          {/if}
                          {#if canAddToLexicon(lint.original, lint.subcategory)}
                            <button
                              type="button"
                              class="feed-dict-btn"
                              onclick={() => addEditorWordToDictionary(lint.original, lint.id)}
                            >
                              + Lexicon
                            </button>
                          {/if}
                        </div>

                        <div class="feed-card-icons">
                          <button
                            type="button"
                            class="note-text-action"
                            title="Dismiss annotation"
                            onclick={() => dismissEditorLint(lint.id)}
                          >
                            Dismiss
                          </button>
                          <button
                            type="button"
                            class="elo-icon-btn"
                            title="Mute this editorial rule"
                            onclick={() => disableEditorRule(lint.rule_name, lint.id)}
                          >
                            ⊘
                          </button>
                        </div>
                      </div>
                    </article>
                  {:else}
                    <button
                      type="button"
                      class="margin-note-card compact"
                      style="--card-accent: {lint.color};"
                      onclick={() => selectAlertFromFeed(lint)}
                    >
                      <div class="compact-top">
                        <span class="compact-cat" style="color: {lint.color};">
                          {lint.category_label.toUpperCase()} · {lint.subcategory}
                        </span>
                        <span class="note-index-tag">#{idx + 1}</span>
                      </div>
                      <div class="compact-diff-row">
                        {#if lint.suggestions[0]}
                          <del class="compact-del">{lint.original}</del>
                          <span class="compact-arrow">→</span>
                          <span class="compact-repl">
                            {lint.suggestions[0].label}
                          </span>
                        {:else}
                          <span class="compact-excerpt">{lint.original}</span>
                        {/if}
                      </div>
                    </button>
                  {/if}
                {/each}
              {/if}
            </div>

            <!-- 4. PINNED BOTTOM "REWRITE ATELIER" DOCK -->
            <div class="rewrite-atelier-dock">
              <div class="atelier-dock-head">
                <div class="atelier-dock-title">
                  <span class="dock-spark">✦</span>
                  <h4>Rewrite Atelier</h4>
                </div>
                <span class="dock-scope-tag">Full Passage</span>
              </div>

              {#if aiRewriteLoading}
                <div class="rewrite-active-progress" role="status" aria-live="polite">
                  <div class="rewrite-progress-row">
                    <progress
                      aria-label="Rewriting text with elo"
                      class="loading-spinner elo-card-spinner"
                    ></progress>
                    <div class="rewrite-progress-copy">
                      <strong>elo is rewriting your manuscript…</strong>
                      <span>Refining clarity, tone, and rhythm</span>
                    </div>
                  </div>
                  <div class="rewrite-shimmer-lines" aria-hidden="true">
                    <span class="shimmer-bar w-92"></span>
                    <span class="shimmer-bar w-76"></span>
                  </div>
                </div>
              {:else if aiRewriteResult}
                <div class="ai-result-card">
                  <p>{aiRewriteResult}</p>
                  <div class="ai-result-actions">
                    <button
                      type="button"
                      class="elo-btn-accept sm"
                      onclick={() => {
                        if (aiRewriteResult) {
                          editorText = aiRewriteResult;
                          aiRewriteResult = null;
                          runEditorCheck();
                        }
                      }}
                    >
                      Replace Manuscript
                    </button>
                    <button
                      type="button"
                      class="elo-btn-dismiss sm"
                      onclick={() => (aiRewriteResult = null)}
                    >
                      Discard
                    </button>
                  </div>
                </div>
              {:else}
                <div class="atelier-preset-chips">
                  <button
                    type="button"
                    class="atelier-chip"
                    onclick={() => runStudioIdeaRewrite("improve")}
                  >
                    ✎ Polish flow
                  </button>
                  <button
                    type="button"
                    class="atelier-chip"
                    onclick={() => runStudioIdeaRewrite("concise")}
                  >
                    🎯 Tighten
                  </button>
                  <button
                    type="button"
                    class="atelier-chip"
                    onclick={() => runStudioIdeaRewrite("confident")}
                  >
                    🤝 Confident
                  </button>
                  <button
                    type="button"
                    class="atelier-chip"
                    onclick={() => runStudioIdeaRewrite("formal")}
                  >
                    🎩 Executive
                  </button>
                  <button
                    type="button"
                    class="atelier-chip"
                    onclick={() => runStudioIdeaRewrite("friendly")}
                  >
                    😊 Warm
                  </button>
                  <button
                    type="button"
                    class="atelier-chip"
                    onclick={() => runStudioIdeaRewrite("bullets")}
                  >
                    • Bullets
                  </button>
                </div>

                {#if aiRewriteError}
                  <p class="ai-error-msg">{aiRewriteError}</p>
                {/if}

                <form
                  class="rewrite-prompt-form"
                  onsubmit={(e) => {
                    e.preventDefault();
                    if (!aiRewriteLoading && studioCustomPrompt.trim()) {
                      const prompt = studioCustomPrompt.trim();
                      studioCustomPrompt = "";
                      runStudioIdeaRewrite("custom", prompt);
                    }
                  }}
                >
                  <input
                    type="text"
                    class="rewrite-prompt-input"
                    placeholder="Direct elo to reshape this draft…"
                    bind:value={studioCustomPrompt}
                  />
                  {#if studioCustomPrompt.trim()}
                    <button type="submit" class="rewrite-prompt-send">→</button>
                  {/if}
                </form>
              {/if}
            </div>
          </aside>
        </main>
      {:else if activeTab === "analytics"}
        <main class="settings-page scroller">
          <div class="settings-container">
            <section class="settings-card apps-folio-card lexicon-folio-card">
              <div class="apps-folio-header">
                <div>
                  <h3>Writing & AI Analytics</h3>
                  <p>
                    Track your prose craft, editorial velocity, and how on-device checks and AI
                    rewrites elevate your writing.
                  </p>
                </div>
                <button type="button" class="ax-strip-link" onclick={loadAllSettings}>
                  ↻ Refresh Metrics
                </button>
              </div>

              {#if writingAnalytics}
                <!-- 1. Hero KPI Strip -->
                <div class="analytics-kpi-grid">
                  <div class="analytics-kpi-card highlight-kpi">
                    <span class="kpi-eyebrow">PROSE CRAFT INDEX</span>
                    <div class="kpi-main-row">
                      <span class="kpi-big-num">{writingAnalytics.prose_craft_score}</span>
                      <span class="kpi-unit">/ 100</span>
                    </div>
                    <span class="kpi-sub">
                      {writingAnalytics.acceptance_rate_pct}% suggestion precision · {writingAnalytics.active_days_14d}d
                      active
                    </span>
                  </div>

                  <div class="analytics-kpi-card">
                    <span class="kpi-eyebrow">EDITORIAL TIME SAVED</span>
                    <div class="kpi-main-row">
                      <span class="kpi-big-num">
                        {writingAnalytics.estimated_minutes_saved >= 60
                          ? `${Math.floor(writingAnalytics.estimated_minutes_saved / 60)}h ${writingAnalytics.estimated_minutes_saved % 60}m`
                          : `${writingAnalytics.estimated_minutes_saved}m`}
                      </span>
                    </div>
                    <span class="kpi-sub">
                      {writingAnalytics.total_accepted_edits + writingAnalytics.total_ai_requests} total
                      interventions
                    </span>
                  </div>

                  <div class="analytics-kpi-card">
                    <span class="kpi-eyebrow">AI CO-PILOT LIFT</span>
                    <div class="kpi-main-row">
                      <span class="kpi-big-num">
                        {writingAnalytics.total_ai_words_refined.toLocaleString()}
                      </span>
                      <span class="kpi-unit">words</span>
                    </div>
                    <span class="kpi-sub">
                      Refined across {writingAnalytics.total_ai_requests.toLocaleString()} AI passes
                    </span>
                  </div>

                  <div class="analytics-kpi-card">
                    <span class="kpi-eyebrow">AI COST EFFICIENCY</span>
                    <div class="kpi-main-row">
                      <span class="kpi-big-num">
                        ${writingAnalytics.avg_cost_per_pass_usd.toFixed(5)}
                      </span>
                      <span class="kpi-unit">/ pass</span>
                    </div>
                    <span class="kpi-sub">
                      ${writingAnalytics.total_cost_usd.toFixed(4)} cumulative spend
                    </span>
                  </div>
                </div>

                <!-- 2. Hybrid Intelligence Balance: Local Engine vs. AI Co-Pilot -->
                <div class="lexicon-subblock">
                  <div class="lexicon-subhead">
                    <h4>Hybrid Intelligence Split</h4>
                    <span class="field-hint">
                      {writingAnalytics.local_vs_ai_pct}% On-Device · {100 -
                        writingAnalytics.local_vs_ai_pct}% Cloud AI Co-Pilot
                    </span>
                  </div>

                  <div
                    class="hybrid-split-bar"
                    role="img"
                    aria-label="On-device vs Cloud AI usage split"
                  >
                    <div
                      class="hybrid-seg local-seg"
                      style="width: {writingAnalytics.local_vs_ai_pct}%;"
                    ></div>
                    <div
                      class="hybrid-seg ai-seg"
                      style="width: {100 - writingAnalytics.local_vs_ai_pct}%;"
                    ></div>
                  </div>

                  <div class="hybrid-cards-row">
                    <div class="hybrid-stat-box">
                      <div class="hybrid-box-head">
                        <span class="hybrid-dot local-dot"></span>
                        <strong>On-Device Harper Engine</strong>
                        <span class="hybrid-pill">0 ms · $0.00</span>
                      </div>
                      <p>
                        {writingAnalytics.total_accepted_edits} accepted inline fixes · {writingAnalytics.vocabulary_shield_words}
                        protected lexicon words · {writingAnalytics.active_rules_count} active rules
                      </p>
                    </div>

                    <div class="hybrid-stat-box">
                      <div class="hybrid-box-head">
                        <span class="hybrid-dot ai-dot"></span>
                        <strong>AI Rewrite Co-Pilot</strong>
                        <span class="hybrid-pill">
                          {writingAnalytics.voice_traits_calibrated} Voice DNA traits
                        </span>
                      </div>
                      <p>
                        {writingAnalytics.total_ai_requests} deep passes · {(
                          writingAnalytics.total_input_tokens + writingAnalytics.total_output_tokens
                        ).toLocaleString()} total tokens processed
                      </p>
                    </div>
                  </div>
                </div>

                <!-- 3. Writing Mastery Pillars & 7-Day Activity + Manuscript Vitals -->
                <div class="analytics-two-col lexicon-subblock">
                  <div class="analytics-col">
                    <div class="lexicon-subhead">
                      <h4>Writing Mastery by Pillar</h4>
                    </div>
                    <div class="pillar-list">
                      {#each writingAnalytics.category_breakdown as item (item.category)}
                        <div class="pillar-row">
                          <div class="pillar-top">
                            <strong>{item.category}</strong>
                            <span class="pillar-score">{item.mastery_pct}%</span>
                          </div>
                          <div class="pillar-track">
                            <div class="pillar-fill" style="width: {item.mastery_pct}%;"></div>
                          </div>
                          <div class="pillar-meta">
                            <span>{item.insight}</span>
                            <span>
                              {item.accepted} applied · {item.dismissed} skipped
                            </span>
                          </div>
                        </div>
                      {/each}
                    </div>
                  </div>

                  <div class="analytics-col">
                    <div class="lexicon-subhead">
                      <h4>7-Day Editorial Cadence</h4>
                    </div>
                    <div class="cadence-chart">
                      {#each writingAnalytics.daily_activity as day (day.day_label)}
                        {@const total = day.ai_passes + day.inline_edits}
                        {@const pct = Math.max(8, Math.round((total / maxDailyActivity) * 100))}
                        <div
                          class="cadence-bar-col"
                          title="{day.day_label}: {day.ai_passes} AI passes, {day.inline_edits} inline edits"
                        >
                          <span class="cadence-val">{total > 0 ? total : ""}</span>
                          <div class="cadence-bar-well">
                            <div class="cadence-bar-fill" style="height: {pct}%;"></div>
                          </div>
                          <span class="cadence-day">{day.day_label}</span>
                        </div>
                      {/each}
                    </div>

                    <div class="manuscript-vitals-box">
                      <span class="learned-edits-heading">ACTIVE MANUSCRIPT VITAL SIGNS</span>
                      <div class="vitals-mini-grid">
                        <div>
                          <strong>{editorLexicalDiversity}%</strong>
                          <span>Vocabulary Variety</span>
                        </div>
                        <div>
                          <strong>{editorAvgSentenceWords} wds</strong>
                          <span>Avg Sentence</span>
                        </div>
                        <div>
                          <strong>{editorReadingTimeLabel}</strong>
                          <span>Reading Time</span>
                        </div>
                        <div>
                          <strong>{editorTone}</strong>
                          <span>Detected Tone</span>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>

                <!-- 4. AI Model Efficiency Ledger (Responsive Table) -->
                {#if writingAnalytics.model_breakdown.length > 0}
                  <div class="lexicon-subblock">
                    <div class="lexicon-subhead">
                      <h4 id="model-ledger-heading">AI Model Efficiency Ledger</h4>
                    </div>
                    <div
                      role="region"
                      aria-labelledby="model-ledger-heading"
                      class="analytics-table-wrapper"
                    >
                      <table class="analytics-model-table">
                        <thead>
                          <tr>
                            <th>Model Slug</th>
                            <th>AI Passes</th>
                            <th>Words Refined</th>
                            <th>Tokens (In / Out)</th>
                            <th>Spend</th>
                          </tr>
                        </thead>
                        <tbody>
                          {#each writingAnalytics.model_breakdown as m (m.model)}
                            <tr>
                              <th>{m.model}</th>
                              <td>{m.requests.toLocaleString()}</td>
                              <td>~{m.words_refined.toLocaleString()}</td>
                              <td>
                                {m.input_tokens.toLocaleString()} / {m.output_tokens.toLocaleString()}
                              </td>
                              <td>${m.cost_usd.toFixed(4)}</td>
                            </tr>
                          {/each}
                        </tbody>
                      </table>
                    </div>
                  </div>
                {/if}

                <!-- 5. Signature Editorial Refinements -->
                {#if writingAnalytics.top_transformations.length > 0}
                  <div class="lexicon-subblock">
                    <div class="lexicon-subhead">
                      <h4>Recent Signature Refinements</h4>
                    </div>
                    <div class="learned-edits-chips">
                      {#each writingAnalytics.top_transformations as ev, idx (idx)}
                        <span class="learned-edit-pill">
                          <span class="ev-cat">{ev.category}</span>
                          <del>{ev.original}</del> → <strong>{ev.replacement}</strong>
                        </span>
                      {/each}
                    </div>
                  </div>
                {/if}
              {/if}
            </section>
          </div>
        </main>
      {:else if activeTab === "apps"}
        <main class="settings-page scroller">
          <div class="settings-container">
            <section class="settings-card apps-folio-card">
              <div class="card-split-header">
                <div>
                  <div class="folio-eyebrow">
                    <span>CONNECTED APPLICATIONS</span>
                    <span class="eyebrow-dot">·</span>
                    <span>
                      {filteredIntegrations.filter((a) => a.enabled).length} OF {filteredIntegrations.length}
                      ENABLED
                    </span>
                    {#if axStatus === "Granted"}
                      <span class="eyebrow-dot">·</span>
                      <span class="ax-granted-inline">✓ ACCESSIBILITY ACTIVE</span>
                    {/if}
                  </div>
                  <h2 class="settings-folio-title">Where “elo” Writes With You</h2>
                  <p>
                    Choose which macOS applications display inline underlines and the floating “elo”
                    companion mark.
                  </p>
                </div>
                <label class="toggle-switch-label">
                  <span>Auto-enable new apps</span>
                  <input
                    type="checkbox"
                    {...{ switch: "" } as Record<string, string>}
                    checked={autoEnableNewApps}
                    onchange={toggleAutoEnableApps}
                  />
                </label>
              </div>

              {#if axStatus !== "Granted"}
                <div class="ax-inline-strip">
                  <div class="ax-strip-left">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <rect
                        x="3"
                        y="11"
                        width="18"
                        height="11"
                        rx="2"
                        stroke="currentColor"
                        stroke-width="1.8"
                      />
                      <path
                        d="M7 11V7a5 5 0 0 1 10 0v4"
                        stroke="currentColor"
                        stroke-width="1.8"
                        stroke-linecap="round"
                      />
                    </svg>
                    <span>
                      macOS Accessibility access is needed to read and replace text in external
                      apps.
                    </span>
                  </div>
                  <div class="ax-strip-actions">
                    <button
                      type="button"
                      class="ax-strip-link primary"
                      onclick={handleGrantAccessibility}
                    >
                      Open System Settings →
                    </button>
                    <button type="button" class="ax-strip-link" onclick={loadAllSettings}>
                      Recheck
                    </button>
                  </div>
                </div>
              {/if}

              <input
                type="search"
                class="settings-search"
                placeholder="Search applications…"
                bind:value={appSearchQuery}
              />

              <div class="app-grid">
                {#each filteredIntegrations as app (app.bundle_id)}
                  <label class="app-row-card" class:is-disabled={!app.enabled}>
                    <div class="app-row-left">
                      <span class="app-monogram" aria-hidden="true">
                        {app.display_name.trim().charAt(0).toUpperCase() || "A"}
                      </span>
                      <div class="app-row-info">
                        <strong>{app.display_name}</strong>
                        {#if appSearchQuery.trim()}
                          <span>{app.bundle_id}</span>
                        {/if}
                      </div>
                    </div>
                    <input
                      type="checkbox"
                      {...{ switch: "" } as Record<string, string>}
                      checked={app.enabled}
                      onchange={(e) =>
                        toggleAppIntegration(
                          app.bundle_id,
                          (e.currentTarget as HTMLInputElement).checked,
                        )}
                    />
                  </label>
                {/each}
              </div>
            </section>
          </div>
        </main>
      {:else if activeTab === "rules"}
        <main class="settings-page scroller">
          <div class="settings-container">
            <section class="settings-card apps-folio-card lexicon-folio-card">
              <!-- English Dialect -->
              <div class="lexicon-subblock first-subblock">
                <div class="lexicon-subhead">
                  <h4>English Dialect</h4>
                </div>
                <div class="dialect-segmented-bar" role="group" aria-label="English Dialect">
                  {#each [{ id: "US", code: "US", label: "American" }, { id: "GB", code: "UK", label: "British" }, { id: "CA", code: "CA", label: "Canadian" }, { id: "AU", code: "AU", label: "Australian" }, { id: "IN", code: "IN", label: "Indian" }] as d}
                    <button
                      type="button"
                      class="dialect-seg-btn"
                      class:active={dialectName === d.id}
                      onclick={() => changeDialect(d.id)}
                    >
                      <span class="dialect-code">{d.code}</span>
                      <span class="dialect-name">{d.label}</span>
                    </button>
                  {/each}
                </div>
              </div>

              <!-- Personal Dictionary -->
              <div class="lexicon-subblock">
                <div class="lexicon-subhead">
                  <h4>
                    Personal Dictionary
                    <span class="lexicon-count-pill">
                      {dictionaryWords.length}
                      {dictionaryWords.length === 1 ? "word" : "words"}
                    </span>
                  </h4>
                  <div class="dict-inline-form">
                    <input
                      type="text"
                      class="settings-search dict-inline-input"
                      placeholder={dictionaryWords.length > 8
                        ? `Search or add to ${dictionaryWords.length} words…`
                        : "Add a name, technical term, or brand word…"}
                      bind:value={newDictWord}
                      onkeydown={(e) => e.key === "Enter" && addNewDictionaryWord()}
                    />
                    <button
                      type="button"
                      class="elo-btn-accept dict-add-btn"
                      onclick={addNewDictionaryWord}
                    >
                      Add Word
                    </button>
                  </div>
                </div>

                <div class="dict-scroll-well">
                  {#if dictionaryWords.length === 0}
                    <span class="dict-empty-hint">
                      No custom words added yet. Words added here are never flagged as spelling
                      errors.
                    </span>
                  {:else if filteredDictionaryWords.length === 0}
                    <span class="dict-empty-hint">
                      No matching word for "{newDictWord.trim()}" — press Enter or click Add Word to
                      save it.
                    </span>
                  {:else}
                    <div class="dict-chips-container">
                      {#each filteredDictionaryWords as word, idx (word)}
                        <span class="dict-chip custom-chip" class:deferred-dict-chip={idx >= 40}>
                          <span class="dict-chip-word">{word}</span>
                          <button
                            type="button"
                            aria-label={`Remove ${word}`}
                            onclick={() => removeDictionaryWord(word)}
                          >
                            ×
                          </button>
                        </span>
                      {/each}
                    </div>
                  {/if}
                </div>
              </div>

              <!-- Editorial & Grammar Rules Grid -->
              <div class="lexicon-subblock rules-subblock">
                <div class="rules-toolbar">
                  <div class="rules-toolbar-left">
                    <h4>Editorial Rules</h4>
                    <div class="rules-filter-pills" role="group" aria-label="Filter rules">
                      <button
                        type="button"
                        class="rule-filter-pill"
                        class:active={ruleFilterMode === "all"}
                        onclick={() => (ruleFilterMode = "all")}
                      >
                        All ({lintRules.length})
                      </button>
                      <button
                        type="button"
                        class="rule-filter-pill"
                        class:active={ruleFilterMode === "enabled"}
                        onclick={() => (ruleFilterMode = "enabled")}
                      >
                        Active ({enabledRulesCount})
                      </button>
                      <button
                        type="button"
                        class="rule-filter-pill"
                        class:active={ruleFilterMode === "disabled"}
                        onclick={() => (ruleFilterMode = "disabled")}
                      >
                        Muted ({disabledRulesCount})
                      </button>
                    </div>
                  </div>
                  <input
                    type="search"
                    class="settings-search rules-search-input"
                    placeholder="Search rules…"
                    bind:value={ruleSearchQuery}
                  />
                </div>

                {#if filteredRules.length === 0}
                  <div class="rules-empty-state">No editorial rules match your current filter.</div>
                {:else}
                  <div class="rules-grid">
                    {#each filteredRules.slice(0, 120) as rule, idx (rule.key)}
                      <label
                        class="rule-card-row"
                        class:is-disabled={!rule.enabled}
                        class:deferred-rule-row={idx >= 16}
                      >
                        <div class="rule-card-info">
                          <strong>{formatRuleDisplayName(rule.key)}</strong>
                          {#if ruleSearchQuery.trim()}
                            <span>{rule.key}</span>
                          {/if}
                        </div>
                        <input
                          type="checkbox"
                          {...{ switch: "" } as Record<string, string>}
                          checked={rule.enabled}
                          onchange={(e) =>
                            toggleLintRule(rule.key, (e.currentTarget as HTMLInputElement).checked)}
                        />
                      </label>
                    {/each}
                  </div>
                {/if}
              </div>
            </section>
          </div>
        </main>
      {:else if activeTab === "voice"}
        <main class="settings-page scroller">
          <div class="settings-container">
            <section class="settings-card apps-folio-card">
              <div class="apps-folio-header">
                <div>
                  <h3>Voice DNA</h3>
                  <p>
                    Paste a writing sample or let Elocutio learn automatically as you accept and
                    dismiss edits.
                  </p>
                </div>
                <label class="toggle-switch-label">
                  <span>Learn from edits</span>
                  <input
                    type="checkbox"
                    {...{ switch: "" } as Record<string, string>}
                    checked={learnFromEdits}
                    onchange={async (e) => {
                      learnFromEdits = await invoke<boolean>("set_learn_from_edits", {
                        enabled: (e.currentTarget as HTMLInputElement).checked,
                      });
                      personalizationDb = await invoke<PersonalizationDbStatus>(
                        "get_personalization_db_status",
                      );
                    }}
                  />
                </label>
              </div>

              {#if personalizationDb}
                <div class="db-ledger-strip">
                  <div class="db-stat-grid">
                    <div class="db-stat-tile">
                      <span class="db-stat-num">{personalizationDb.learned_events_count}</span>
                      <span class="db-stat-label">Learned Edits</span>
                    </div>
                    <div class="db-stat-tile">
                      <span class="db-stat-num">{dictionaryWords.length}</span>
                      <span class="db-stat-label">Custom Words</span>
                    </div>
                    <div class="db-stat-tile">
                      <span class="db-stat-num">{disabledRulesCount}</span>
                      <span class="db-stat-label">Muted Rules</span>
                    </div>
                    <div class="db-stat-tile">
                      <span class="db-stat-num">{voiceTraits.length}</span>
                      <span class="db-stat-label">Voice Traits</span>
                    </div>
                  </div>

                  {#if personalizationDb.recent_learned_edits.length > 0}
                    <div class="learned-edits-list">
                      <span class="learned-edits-heading">RECENTLY LEARNED</span>
                      <div class="learned-edits-chips">
                        {#each personalizationDb.recent_learned_edits.slice(0, 8) as ev}
                          <span
                            class="learned-edit-pill"
                            class:is-dismissed={ev.kind === "dismissed"}
                          >
                            <span class="ev-cat">{ev.category}</span>
                            {#if ev.kind === "dismissed"}
                              <span class="ev-pair">Kept "{ev.original}"</span>
                            {:else}
                              <span class="ev-pair"
                                >"{ev.original}" → "{ev.replacement || "∅"}"</span
                              >
                            {/if}
                          </span>
                        {/each}
                      </div>
                    </div>
                  {/if}
                </div>
              {/if}

              <textarea
                class="voice-sample-box"
                rows="4"
                bind:value={voiceSampleText}
                placeholder="Paste a sample of your own writing here to extract sentence cadence, formality, and vocabulary traits…"
              ></textarea>
              <div class="perm-actions">
                <button type="button" class="elo-btn-accept" onclick={handleLearnVoice}>
                  Analyze & Learn Voice
                </button>
                {#if voiceStatusMsg}
                  <p class="status-feedback">{voiceStatusMsg}</p>
                {/if}
              </div>

              {#if voiceTraits.length > 0}
                <div class="lexicon-subblock">
                  <h4>Learned Voice Traits</h4>
                  <div class="app-grid">
                    {#each voiceTraits as t}
                      <div class="app-row-card">
                        <div class="app-row-info">
                          <strong>{t.label || formatRuleDisplayName(t.key)}</strong>
                          <span>{t.summary || t.evidence?.[0] || String(t.value ?? "")}</span>
                        </div>
                        <span class="perm-badge granted">
                          {Math.round(t.confidence * 100)}%
                        </span>
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}
            </section>
          </div>
        </main>
      {:else if activeTab === "ai"}
        <main class="settings-page scroller">
          <div class="settings-container">
            <section class="settings-card apps-folio-card lexicon-folio-card">
              <div class="apps-folio-header">
                <div>
                  <h3>Privacy & AI Engine</h3>
                  <p>
                    Select an on-device or cloud AI provider, pick a model slug, and manage your
                    token budget.
                  </p>
                </div>
                <label class="toggle-switch-label">
                  <span>{localOnlyMode ? "Local-Only ON" : "Cloud AI Enabled"}</span>
                  <input
                    type="checkbox"
                    {...{ switch: "" } as Record<string, string>}
                    checked={localOnlyMode}
                    onchange={toggleLocalOnly}
                  />
                </label>
              </div>

              <!-- AI Provider & Model Configuration -->
              <div class="lexicon-subblock">
                <div class="lexicon-subhead">
                  <h4>Provider & Model</h4>
                  <button
                    type="button"
                    class="ax-strip-link"
                    onclick={() => refreshPresetModels(selectedPresetId, providerSettings.base_url)}
                  >
                    {fetchingModels
                      ? "Fetching models…"
                      : `↻ Refresh Models (${presetModelSlugs.length})`}
                  </button>
                </div>

                <div class="ai-form-grid">
                  <div class="form-field">
                    <label for="provider-preset-select">
                      <span>Provider Preset</span>
                    </label>
                    <select
                      id="provider-preset-select"
                      name="provider_preset"
                      value={selectedPresetId}
                      onchange={onPresetSelectChange}
                    >
                      {#each PROVIDER_PRESETS as preset (preset.id)}
                        <option value={preset.id}>
                          {preset.name} ({preset.tag})
                        </option>
                      {/each}
                      <option value="custom">Custom OpenAI-Compatible Endpoint</option>
                    </select>
                  </div>

                  <div class="form-field">
                    <label for="model-slug-select">
                      <span>
                        Model Slug
                        <em class="field-hint">{presetModelSlugs.length} available</em>
                      </span>
                    </label>
                    <select
                      id="model-slug-select"
                      name="model_slug"
                      value={customModelSlugMode ? "__custom__" : providerSettings.model}
                      onchange={onModelSlugSelectChange}
                    >
                      {#each presetModelSlugs as m (m.slug)}
                        <option value={m.slug}>
                          {m.slug} — {m.badge}
                        </option>
                      {/each}
                      {#if providerSettings.model && !presetModelSlugs.some((m) => m.slug === providerSettings.model)}
                        <option value={providerSettings.model}>
                          {providerSettings.model} — Active
                        </option>
                      {/if}
                      <option value="__custom__">Custom model slug…</option>
                    </select>
                  </div>

                  <div class="form-field">
                    <label for="provider-base-url">
                      <span>Base URL</span>
                    </label>
                    <input
                      id="provider-base-url"
                      name="base_url"
                      type="url"
                      autocorrect="off"
                      autocapitalize="off"
                      spellcheck="false"
                      bind:value={providerSettings.base_url}
                      placeholder="https://openrouter.ai/api/v1"
                    />
                  </div>

                  <div class="form-field">
                    <label for="provider-api-key">
                      <span>
                        API Key
                        {#if selectedPresetId === "ollama_local" || selectedPresetId === "apple_foundation"}
                          <em class="field-hint">(Optional — On-Device)</em>
                        {:else if providerSettings.key_present}
                          <em class="field-hint encrypted-ok">
                            ✓ Encrypted ({providerSettings.key_hint || "AES-256"})
                          </em>
                        {:else}
                          <em class="field-hint">(Not set)</em>
                        {/if}
                      </span>
                    </label>
                    <input
                      id="provider-api-key"
                      name="api_key"
                      type="password"
                      autocomplete="off"
                      bind:value={apiKeyInput}
                      placeholder={providerSettings.key_present
                        ? "•••••••••••••••• (leave blank to keep encrypted key)"
                        : selectedPresetId === "ollama_local" ||
                            selectedPresetId === "apple_foundation"
                          ? "No key required for local endpoint"
                          : "Enter API key (encrypted locally with AES-256)…"}
                    />
                  </div>

                  {#if customModelSlugMode}
                    <div class="form-field">
                      <label for="custom-model-slug">
                        <span>Custom Model Identifier (Slug)</span>
                      </label>
                      <input
                        id="custom-model-slug"
                        name="custom_model_slug"
                        type="text"
                        autocorrect="off"
                        autocapitalize="off"
                        spellcheck="false"
                        bind:value={providerSettings.model}
                        placeholder="e.g., anthropic/claude-haiku-4.5"
                      />
                    </div>
                  {/if}

                  <div class="form-field" class:span-full={!customModelSlugMode}>
                    <label for="custom-editorial-instruction">
                      <span>Custom Editorial Instruction</span>
                    </label>
                    <input
                      id="custom-editorial-instruction"
                      name="custom_instruction"
                      type="text"
                      bind:value={customInstruction}
                      placeholder="e.g., Prefer active verbs and concise sentences."
                    />
                  </div>
                </div>
              </div>

              <!-- Token Usage & Budget -->
              <div class="lexicon-subblock">
                <div class="lexicon-subhead">
                  <h4>Token Usage & Budget</h4>
                </div>

                <div class="db-stat-grid">
                  <div class="db-stat-tile">
                    <span class="db-stat-num">{usageSummary.requests.toLocaleString()}</span>
                    <span class="db-stat-label">AI Requests</span>
                  </div>
                  <div class="db-stat-tile">
                    <span class="db-stat-num">{usageSummary.input_tokens.toLocaleString()}</span>
                    <span class="db-stat-label">Input Tokens</span>
                  </div>
                  <div class="db-stat-tile">
                    <span class="db-stat-num">{usageSummary.output_tokens.toLocaleString()}</span>
                    <span class="db-stat-label">Output Tokens</span>
                  </div>
                  <div class="db-stat-tile">
                    <span class="db-stat-num">${usageSummary.cost_usd.toFixed(4)}</span>
                    <span class="db-stat-label">Estimated Spend</span>
                  </div>
                </div>

                {#if budgetInput.trim() && Number(budgetInput) > 0}
                  <div class="budget-progress-wrap">
                    <div class="budget-progress-labels">
                      <span>Budget Used</span>
                      <span>
                        ${usageSummary.cost_usd.toFixed(4)} / ${Number(budgetInput).toFixed(2)}
                      </span>
                    </div>
                    <progress
                      class="budget-progress-bar"
                      max={Number(budgetInput)}
                      value={Math.min(usageSummary.cost_usd, Number(budgetInput))}
                    ></progress>
                  </div>
                {/if}

                <div class="ai-form-grid">
                  <label class="form-field">
                    <span>Monthly Budget Alert (USD)</span>
                    <input
                      type="number"
                      min="0"
                      step="0.50"
                      bind:value={budgetInput}
                      placeholder="e.g., 5.00 (optional)"
                    />
                  </label>
                  <label class="form-field">
                    <span>Per-Request Cost Cap (USD)</span>
                    <input
                      type="number"
                      min="0"
                      step="0.01"
                      bind:value={costCapInput}
                      placeholder="e.g., 0.05 (optional hard ceiling)"
                    />
                  </label>
                </div>

                <div class="perm-actions">
                  <button type="button" class="elo-btn-accept" onclick={saveProviderSettings}>
                    Save Provider Configuration
                  </button>
                  <button type="button" class="elo-btn-secondary" onclick={testProviderConnection}>
                    Test Connection
                  </button>
                  {#if providerStatusMsg}
                    <p class="status-feedback" class:is-error={providerStatusIsError}>
                      {providerStatusMsg}
                    </p>
                  {/if}
                </div>
              </div>
            </section>
          </div>
        </main>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* =========================================================================
     MODE A: SYSTEM-WIDE "elo" OVERLAY STYLES
     ========================================================================= */
  .elo-overlay-stage {
    position: fixed;
    inset: 0;
    width: 100vw;
    height: 100vh;
    background: transparent;
    pointer-events: none;
    overflow: hidden;
    user-select: none;
  }

  .elo-field-clip {
    position: fixed;
    overflow: hidden;
    pointer-events: none;
    z-index: 9997;
  }

  .elo-word-underline {
    position: absolute;
    background: transparent;
    border: none;
    border-bottom: 2.5px solid var(--underline-color, #e5484d);
    border-radius: 2px;
    padding: 0;
    margin: 0;
    cursor: pointer;
    pointer-events: auto;
    transition: background-color 120ms ease;
  }

  .elo-word-underline:hover,
  .elo-word-underline.is-active {
    background: color-mix(in srgb, var(--underline-color, #e5484d) 18%, transparent);
    border-bottom-width: 3px;
  }

  /* Teardrop Dual-Action & Hover-Expandable "elo" Pill (Images 1, 2, 4) */
  .elo-teardrop-widget {
    position: fixed;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 36px;
    padding: 3px 5px 3px 7px;
    background: #ffffff;
    border: 1px solid rgba(23, 23, 23, 0.14);
    /* Asymmetric speech-bubble / teardrop bottom-left corner matching Image 1 */
    border-radius: 20px 20px 20px 4px;
    box-shadow:
      0 6px 20px rgba(23, 23, 23, 0.14),
      0 1px 4px rgba(25, 31, 112, 0.08);
    cursor: grab;
    pointer-events: auto;
    z-index: 9998;
    transition:
      box-shadow 140ms ease,
      border-color 140ms ease,
      border-radius 140ms ease;
  }

  .elo-teardrop-widget.is-expanded {
    border-radius: 999px;
    padding: 3px 5px 3px 8px;
    border-color: rgba(25, 31, 112, 0.25);
    box-shadow:
      0 10px 28px rgba(23, 23, 23, 0.18),
      0 2px 8px rgba(25, 31, 112, 0.12);
  }

  .elo-teardrop-widget.is-rewriting {
    border-color: rgba(16, 185, 129, 0.55);
    animation: elo-widget-rewrite-pulse 1.4s ease-in-out infinite;
  }

  @keyframes elo-widget-rewrite-pulse {
    0%,
    100% {
      box-shadow:
        0 6px 20px rgba(23, 23, 23, 0.14),
        0 0 0 2px rgba(16, 185, 129, 0.2),
        0 0 14px rgba(25, 31, 112, 0.16);
    }
    50% {
      box-shadow:
        0 8px 26px rgba(16, 185, 129, 0.28),
        0 0 0 3.5px rgba(16, 185, 129, 0.38),
        0 0 20px rgba(25, 31, 112, 0.26);
    }
  }

  .elo-bulb-busy-wrap {
    position: relative;
    width: 28px;
    height: 28px;
    display: inline-grid;
    place-items: center;
  }

  .elo-bulb-busy-wrap :global(svg) {
    animation: elo-bulb-spark-pulse 1s ease-in-out infinite;
  }

  @keyframes elo-bulb-spark-pulse {
    0%,
    100% {
      transform: scale(0.92);
      opacity: 0.9;
    }
    50% {
      transform: scale(1.08);
      opacity: 1;
    }
  }

  /* Modern semantic indeterminate <progress> spinner (modern-web-guidance `spinner`) */
  @property --arc-start {
    syntax: "<angle>";
    inherits: false;
    initial-value: 0deg;
  }
  @property --arc-end {
    syntax: "<angle>";
    inherits: false;
    initial-value: 0deg;
  }
  @property --progress-dash-animation {
    syntax: "*";
    inherits: false;
    initial-value: progress-dash 3s ease-in-out infinite;
  }

  progress.loading-spinner:indeterminate::-webkit-progress-bar {
    display: none;
    background: none;
  }
  progress.loading-spinner:indeterminate::-webkit-progress-value {
    display: none;
    background: none;
  }
  progress.loading-spinner:indeterminate::-moz-progress-bar {
    display: none;
    background: none;
  }
  progress.loading-spinner:indeterminate::slider-fill {
    display: none;
    background: none;
  }

  progress.loading-spinner:indeterminate {
    --_from: calc(90deg + var(--arc-start, 0deg));
    --_to: calc(90deg + var(--arc-end, 158deg));
    --size: 26px;
    --thickness: 2.5px;
    --spinner-color: #191f70;
    --track-color: rgba(16, 185, 129, 0.24);
    --spinner-duration: 1.1s;
    --_used-spinner-duration: var(--spinner-duration);

    position: relative;
    width: var(--size);
    height: var(--size);
    border-radius: 50%;
    appearance: none;
    flex-shrink: 0;

    background: conic-gradient(
        from var(--_from),
        var(--spinner-color) calc(var(--_to) - var(--_from)),
        transparent 0
      )
      var(--track-color);

    @supports (background-clip: border-area) {
      background-clip: border-area;
      border: var(--thickness) solid transparent;
      background-origin: border-box;
    }

    @supports not (background-clip: border-area) {
      --clip-boundary: calc(100% - var(--thickness));
      mask-image: radial-gradient(
        farthest-side,
        transparent var(--clip-boundary),
        black var(--clip-boundary)
      );
      border: 0;
    }

    animation:
      progress-spin var(--_used-spinner-duration) linear infinite,
      var(--progress-dash-animation, none);
  }

  progress.loading-spinner.elo-bulb-spinner:indeterminate {
    position: absolute;
    inset: 1px;
    --size: 26px;
    --thickness: 2.2px;
    --spinner-color: #10b981;
    --track-color: rgba(25, 31, 112, 0.16);
  }

  progress.loading-spinner.elo-card-spinner:indeterminate {
    --size: 22px;
    --thickness: 2.5px;
    --spinner-color: #191f70;
    --track-color: rgba(25, 31, 112, 0.14);
  }

  @keyframes -global-progress-spin {
    to {
      rotate: 1turn;
    }
  }

  @keyframes -global-progress-dash {
    from {
      --arc-start: 0deg;
      --arc-end: 3deg;
    }
    50% {
      --arc-start: 100deg;
      --arc-end: 358deg;
    }
    to {
      --arc-start: 360deg;
      --arc-end: 363deg;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .loading-spinner {
      --_used-spinner-duration: 6s;
    }
    .elo-teardrop-widget.is-rewriting,
    .elo-bulb-busy-wrap :global(svg),
    .shimmer-bar {
      animation-duration: 4s;
    }
  }

  .elo-teardrop-widget.studio-inline-pos {
    position: absolute;
    right: 22px;
    bottom: 18px;
  }

  .elo-sub-action-btn {
    border: none;
    background: transparent;
    width: 28px;
    height: 28px;
    border-radius: 999px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
    color: #525668;
    transition: background 120ms ease;
  }

  .elo-sub-action-btn:hover,
  .elo-sub-action-btn.is-hovered {
    background: #eceef4;
  }

  .elo-brand-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    overflow: hidden;
  }

  .elo-brand-mark-img {
    display: block;
    object-fit: contain;
    user-select: none;
    pointer-events: none;
    flex-shrink: 0;
  }

  .elo-pill-wink-mark {
    border: none;
    background: transparent;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0 2px;
    flex-shrink: 0;
    cursor: pointer;
  }

  .elo-count-badge-btn {
    position: relative;
    border: none;
    min-width: 28px;
    height: 28px;
    padding: 0 8px;
    border-radius: 999px 999px 999px 4px;
    background: #e5484d;
    color: #ffffff;
    font-size: 13px;
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }

  .elo-count-badge-btn.style-only {
    background: #5b43d6;
    border-radius: 999px;
  }

  .elo-count-badge-btn.zero {
    background: transparent;
    padding: 0;
  }

  .elo-badge-corner-dot {
    position: absolute;
    top: -2px;
    right: -2px;
    width: 9px;
    height: 9px;
    border-radius: 999px;
    background: #5b43d6;
    border: 1.5px solid #ffffff;
  }

  /* Dark Slate Tooltip Above "elo" Pill ("Rewrite with elo", Image 4) */
  .elo-dark-tooltip {
    position: absolute;
    bottom: calc(100% + 9px);
    right: 6px;
    background: #2f3545;
    color: #ffffff;
    font-size: 12.5px;
    font-weight: 500;
    padding: 6px 12px;
    border-radius: 7px;
    white-space: nowrap;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.22);
    pointer-events: none;
  }

  .elo-dark-tooltip::after {
    content: "";
    position: absolute;
    top: 100%;
    left: 50%;
    transform: translateX(-50%);
    border-width: 5px;
    border-style: solid;
    border-color: #2f3545 transparent transparent transparent;
  }

  .elo-dark-tooltip.is-rewriting-tip {
    background: linear-gradient(135deg, #191f70 0%, #0f7b55 100%);
    font-weight: 600;
  }

  .elo-dark-tooltip.is-rewriting-tip::after {
    border-color: #144d63 transparent transparent transparent;
  }

  .elo-power-menu {
    position: absolute;
    bottom: calc(100% + 10px);
    left: 0;
    min-width: 200px;
    background: #ffffff;
    border: 1px solid rgba(23, 23, 23, 0.12);
    border-radius: 10px;
    box-shadow: 0 12px 28px rgba(23, 23, 23, 0.18);
    padding: 5px;
    display: flex;
    flex-direction: column;
    z-index: 30;
  }

  .elo-power-menu button {
    border: none;
    background: transparent;
    text-align: left;
    padding: 7px 10px;
    font-size: 12.5px;
    color: #171717;
    border-radius: 6px;
    cursor: pointer;
  }

  .elo-power-menu button:hover {
    background: #f7f7f3;
  }

  /* Compact Inline Quick-Fix Mini-Popover (Image 3) */
  .elo-mini-popover {
    position: fixed;
    background: #ffffff;
    color: #171717;
    border: 1px solid rgba(23, 23, 23, 0.1);
    border-radius: 14px;
    box-shadow:
      0 14px 36px rgba(23, 23, 23, 0.16),
      0 2px 8px rgba(23, 23, 23, 0.08);
    padding: 13px 15px;
    pointer-events: auto;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .elo-mini-popover.studio-pos {
    position: absolute;
    width: 248px;
  }

  .mini-headline {
    font-size: 13px;
    color: #687082;
    font-weight: 500;
  }

  .mini-replacement-btn {
    border: none;
    background: transparent;
    padding: 3px 0 5px;
    text-align: left;
    cursor: pointer;
    font-size: 18px;
    font-weight: 700;
    color: #2563eb;
    line-height: 1.25;
  }

  .mini-replacement-btn:hover .mini-repl-highlight {
    text-decoration: underline;
  }

  .mini-action-row {
    border: none;
    background: transparent;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 5px 2px;
    border-radius: 6px;
    cursor: pointer;
    color: #5b6275;
    font-size: 13.5px;
    font-weight: 500;
    text-align: left;
  }

  .mini-action-row:hover {
    background: #f5f6f9;
    color: #171717;
  }

  /* "What do you want to do?" Generative Rewrite Card (Image 5) */
  .elo-rewrite-card {
    position: fixed;
    background: #ffffff;
    color: #171717;
    border: 1px solid rgba(23, 23, 23, 0.12);
    border-radius: 14px;
    box-shadow:
      0 20px 48px rgba(23, 23, 23, 0.2),
      0 4px 14px rgba(25, 31, 112, 0.1);
    pointer-events: auto;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .elo-rewrite-card.studio-pos {
    position: absolute;
  }

  .rewrite-card-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    border-bottom: 1px solid rgba(23, 23, 23, 0.08);
  }

  .rewrite-brand-left {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .rewrite-top-icons {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .rewrite-ideas-body {
    padding: 16px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .rewrite-ideas-body h3 {
    margin: 0;
    font-size: 15.5px;
    font-weight: 700;
    color: #171717;
  }

  .rewrite-sub {
    margin: -4px 0 2px;
    font-size: 13px;
    color: #687082;
  }

  .rewrite-idea-list {
    display: flex;
    flex-direction: column;
  }

  .rewrite-active-progress {
    background: #f7f7f3;
    border: 1px solid rgba(25, 31, 112, 0.14);
    border-radius: 10px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .rewrite-progress-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .rewrite-progress-copy {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .rewrite-progress-copy strong {
    font-size: 13px;
    color: #191f70;
    font-weight: 700;
  }

  .rewrite-progress-copy span {
    font-size: 11.5px;
    color: #687082;
  }

  .rewrite-shimmer-lines {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .shimmer-bar {
    display: block;
    height: 6px;
    border-radius: 999px;
    background: linear-gradient(
      90deg,
      rgba(25, 31, 112, 0.08) 0%,
      rgba(16, 185, 129, 0.28) 50%,
      rgba(25, 31, 112, 0.08) 100%
    );
    background-size: 200% 100%;
    animation: elo-shimmer-slide 1.3s linear infinite;
  }

  .shimmer-bar.w-92 {
    width: 92%;
  }

  .shimmer-bar.w-76 {
    width: 76%;
  }

  @keyframes elo-shimmer-slide {
    from {
      background-position: 200% 0;
    }
    to {
      background-position: -200% 0;
    }
  }

  .rewrite-idea-row {
    border: none;
    border-bottom: 1px solid rgba(23, 23, 23, 0.07);
    background: transparent;
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 10px 4px;
    font-size: 14px;
    font-weight: 500;
    color: #171717;
    cursor: pointer;
    text-align: left;
  }

  .rewrite-idea-row:hover:not(:disabled) {
    background: #f7f8fb;
  }

  .rewrite-idea-row:disabled {
    opacity: 0.55;
    cursor: wait;
  }

  .rewrite-more-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .rewrite-more-row .flex-1 {
    flex: 1;
    border-bottom: none;
  }

  .rewrite-extra-ideas {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 6px 0 4px;
  }

  .rewrite-chip {
    border: 1px solid rgba(25, 31, 112, 0.18);
    background: #f7f7f3;
    color: #191f70;
    font-size: 12px;
    font-weight: 600;
    padding: 5px 10px;
    border-radius: 999px;
    cursor: pointer;
  }

  .rewrite-chip:hover:not(:disabled) {
    background: #191f70;
    color: #ffffff;
  }

  .rewrite-chip:disabled {
    opacity: 0.55;
    cursor: wait;
  }

  .rewrite-prompt-form {
    position: relative;
    margin-top: 4px;
  }

  .rewrite-prompt-input {
    width: 100%;
    padding: 10px 34px 10px 13px;
    border: 1px solid rgba(23, 23, 23, 0.16);
    border-radius: 9px;
    font-size: 13.5px;
    color: #171717;
    background: #ffffff;
    box-shadow: 0 2px 6px rgba(23, 23, 23, 0.05);
    outline: none;
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
    pointer-events: auto;
  }

  .rewrite-prompt-input:focus {
    border-color: #191f70;
  }

  .rewrite-prompt-send {
    position: absolute;
    right: 6px;
    top: 50%;
    transform: translateY(-50%);
    border: none;
    background: #191f70;
    color: #ffffff;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 12px;
  }

  .rewrite-preview-body {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .rewrite-preview-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .rewrite-mode-pill {
    font-size: 12px;
    font-weight: 700;
    color: #191f70;
    background: rgba(25, 31, 112, 0.09);
    padding: 3px 9px;
    border-radius: 999px;
  }

  .rewrite-engine-tag {
    font-size: 11px;
    color: #687082;
  }

  .rewrite-preview-box {
    background: #f7f7f3;
    border: 1px solid rgba(23, 23, 23, 0.09);
    border-radius: 9px;
    padding: 11px 12px;
    font-size: 13.5px;
    line-height: 1.5;
    color: #171717;
    max-height: 170px;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .rewrite-preview-actions {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .rewrite-err {
    margin: 0;
    font-size: 12px;
    color: #e5484d;
  }

  /* Full Multi-Alert Suggestion Card */
  .elo-floating-card {
    position: fixed;
    background: #ffffff;
    color: #171717;
    border: 1px solid rgba(23, 23, 23, 0.12);
    border-radius: 14px;
    box-shadow:
      0 20px 48px rgba(23, 23, 23, 0.22),
      0 4px 14px rgba(25, 31, 112, 0.1);
    padding: 12px 14px 10px;
    pointer-events: auto;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .elo-card-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .elo-card-brand {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .elo-card-brand-name {
    font-size: 14px;
    font-weight: 700;
    color: #191f70;
    letter-spacing: -0.02em;
  }

  .elo-card-app-pill {
    font-size: 11px;
    font-weight: 500;
    color: #64687a;
    background: #f7f7f3;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid rgba(23, 23, 23, 0.08);
  }

  .elo-card-score-pill {
    font-size: 11px;
    font-weight: 700;
    color: var(--score-accent, #191f70);
    background: color-mix(in srgb, var(--score-accent, #191f70) 11%, #ffffff);
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--score-accent, #191f70) 24%, transparent);
    font-variant-numeric: tabular-nums;
  }

  .elo-card-top-actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .elo-card-stepper {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-right: 4px;
  }

  .elo-step-counter {
    font-size: 11px;
    font-weight: 600;
    color: #64687a;
    padding: 0 4px;
  }

  .elo-icon-btn {
    border: none;
    background: transparent;
    color: #525668;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
  }

  .elo-icon-btn:hover {
    background: rgba(25, 31, 112, 0.08);
    color: #191f70;
  }

  .elo-active-alert-box {
    border: 1.5px solid #191f70;
    border-radius: 10px;
    padding: 12px;
    background: #ffffff;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .elo-alert-cat-row {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    color: #3b3f52;
  }

  .elo-cat-dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    flex-shrink: 0;
  }

  .elo-context-sentence {
    font-size: 14px;
    line-height: 1.48;
    color: #171717;
    background: #f7f7f3;
    padding: 8px 10px;
    border-radius: 8px;
    word-break: break-word;
  }

  .ctx-muted {
    color: #4f5366;
  }

  .ctx-del {
    color: #e5484d;
    text-decoration: line-through;
    margin: 0 2px;
    font-weight: 500;
  }

  .ctx-ins {
    color: #0f7b55;
    text-decoration: none;
    font-weight: 700;
    background: rgba(16, 185, 129, 0.14);
    padding: 1px 5px;
    border-radius: 4px;
    margin: 0 2px;
  }

  .elo-alert-explanation {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.42;
    color: #525668;
  }

  .elo-alert-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-top: 2px;
  }

  .elo-primary-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }

  .elo-btn-accept {
    border: none;
    background: #191f70;
    color: #ffffff;
    font-size: 12.5px;
    font-weight: 600;
    padding: 6px 13px;
    border-radius: 7px;
    cursor: pointer;
    transition: background 120ms ease;
  }

  .elo-btn-accept:hover {
    background: #121757;
  }

  .elo-btn-accept.sm {
    padding: 5px 10px;
    font-size: 12px;
  }

  .elo-btn-alt {
    border: 1px solid rgba(25, 31, 112, 0.25);
    background: rgba(25, 31, 112, 0.06);
    color: #191f70;
    font-size: 12px;
    font-weight: 600;
    padding: 5px 10px;
    border-radius: 7px;
    cursor: pointer;
  }

  .elo-btn-dismiss {
    border: none;
    background: transparent;
    color: #525668;
    font-size: 12.5px;
    font-weight: 500;
    padding: 6px 10px;
    border-radius: 7px;
    cursor: pointer;
  }

  .elo-btn-dismiss:hover {
    background: rgba(23, 23, 23, 0.06);
    color: #171717;
  }

  .elo-btn-dismiss.sm {
    padding: 5px 8px;
    font-size: 12px;
  }

  .elo-more-wrap {
    position: relative;
  }

  .elo-more-dropdown {
    position: absolute;
    right: 0;
    bottom: 30px;
    min-width: 210px;
    background: #ffffff;
    border: 1px solid rgba(23, 23, 23, 0.14);
    border-radius: 9px;
    box-shadow: 0 10px 24px rgba(23, 23, 23, 0.16);
    padding: 4px;
    display: flex;
    flex-direction: column;
    z-index: 20;
  }

  .elo-more-dropdown button {
    border: none;
    background: transparent;
    text-align: left;
    padding: 7px 10px;
    font-size: 12px;
    color: #171717;
    border-radius: 6px;
    cursor: pointer;
  }

  .elo-more-dropdown button:hover {
    background: #f7f7f3;
  }

  .elo-compact-alert-list {
    max-height: 112px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-top: 1px solid rgba(23, 23, 23, 0.08);
    padding-top: 6px;
  }

  .elo-compact-alert-row {
    border: none;
    background: transparent;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 7px;
    cursor: pointer;
    text-align: left;
    font-size: 12.5px;
    color: #171717;
  }

  .elo-compact-alert-row:hover {
    background: #f7f7f3;
  }

  .compact-orig {
    font-weight: 600;
  }

  .compact-sep {
    color: #9ca0b0;
  }

  .compact-sub {
    color: #525668;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .elo-clean-state {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px;
    background: #f7f7f3;
    border-radius: 10px;
  }

  .elo-clean-badge {
    width: 32px;
    height: 32px;
    border-radius: 999px;
    background: #10b981;
    color: #ffffff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
  }

  .elo-clean-state p {
    margin: 3px 0 0;
    font-size: 12px;
    color: #525668;
  }

  .elo-card-cat-footer {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 6px;
    padding-top: 6px;
    border-top: 1px solid rgba(23, 23, 23, 0.08);
  }

  .elo-cat-footer-item {
    border: none;
    background: transparent;
    padding: 3px 2px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 4px;
    text-align: left;
  }

  .elo-cat-track {
    display: block;
    height: 3.5px;
    width: 100%;
    background: #ececf0;
    border-radius: 999px;
    overflow: hidden;
  }

  .elo-cat-bar {
    display: block;
    height: 100%;
    border-radius: 999px;
    transition: width 220ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  .elo-cat-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 3px;
    width: 100%;
  }

  .elo-cat-name {
    font-size: 10.5px;
    color: #525668;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .elo-cat-count {
    font-size: 10px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .elo-cat-footer-item.is-selected .elo-cat-name {
    color: #191f70;
    font-weight: 700;
  }

  /* =========================================================================
     MODE B: ELOCUTIO EDITORIAL ATELIER (WARM LITERARY INK & PAPER)
     ========================================================================= */
  .studio-shell.atelier-shell {
    display: flex;
    flex-direction: row;
    height: 100dvh;
    width: 100dvw;
    background: #efece4;
    color: #18181b;
    overflow: hidden;
  }

  /* 1. Left Midnight Indigo Atelier Rail */
  .atelier-rail {
    width: 84px;
    background: #191f70;
    color: #ffffff;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: space-between;
    padding: 16px 8px 14px;
    flex-shrink: 0;
    border-right: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow: 4px 0 18px rgba(25, 31, 112, 0.14);
    z-index: 10;
  }

  .atelier-rail-top {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    width: 100%;
  }

  .atelier-monogram-btn {
    border: none;
    background: transparent;
    color: #ffffff;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 4px;
    cursor: pointer;
  }

  .atelier-monogram-word {
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.02em;
    color: rgba(255, 255, 255, 0.92);
  }

  .atelier-rail-divider {
    width: 44px;
    height: 1px;
    background: rgba(255, 255, 255, 0.16);
    margin: 2px 0;
  }

  .atelier-rail-nav {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 100%;
  }

  .atelier-rail-btn {
    position: relative;
    width: 68px;
    border: 1px solid transparent;
    background: transparent;
    color: rgba(255, 255, 255, 0.72);
    padding: 8px 4px 7px;
    border-radius: 12px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    transition:
      background 140ms ease,
      color 140ms ease,
      border-color 140ms ease;
  }

  .atelier-rail-btn:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #ffffff;
  }

  .atelier-rail-btn.active {
    background: #fbf9f5;
    color: #191f70;
    border-color: rgba(255, 255, 255, 0.35);
    box-shadow: 0 4px 12px rgba(10, 14, 56, 0.28);
  }

  .rail-svg {
    display: block;
    flex-shrink: 0;
  }

  .rail-label {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }

  .rail-badge {
    position: absolute;
    top: 4px;
    right: 5px;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 999px;
    background: #c84b31;
    color: #ffffff;
    font-size: 10px;
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .rail-badge.warn {
    background: #d97706;
  }

  .atelier-rail-bottom {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding-top: 8px;
    border-top: 1px solid rgba(255, 255, 255, 0.14);
    width: 100%;
  }

  .rail-companion-toggle {
    color: rgba(255, 255, 255, 0.78);
  }

  .rail-companion-icon-wrap {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .rail-status-led {
    position: absolute;
    right: -3px;
    bottom: -2px;
    width: 7px;
    height: 7px;
    border-radius: 999px;
    background: #94a3b8;
    border: 1.5px solid #191f70;
  }

  .rail-status-led.is-on {
    background: #34d399;
  }

  /* 2. Warm Stone Desk & 2-Pane Editorial Atelier Grid */
  .atelier-desk {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: #efece4;
    overflow: hidden;
  }

  .atelier-editor-grid {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(380px, 1.45fr) minmax(330px, 400px);
    gap: 18px;
    padding: 18px 20px;
    min-height: 0;
    overflow: hidden;
  }

  /* Center Pane: Raised Warm Ivory Manuscript Sheet */
  .manuscript-folio {
    container-type: inline-size;
    container-name: manuscript-folio;
    display: flex;
    flex-direction: column;
    background: #fbf9f5;
    border: 1px solid #dfdbd0;
    border-radius: 18px;
    box-shadow:
      0 12px 32px rgba(24, 24, 27, 0.06),
      0 2px 6px rgba(25, 31, 112, 0.04);
    min-height: 0;
    overflow: hidden;
  }

  .folio-header {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    padding: 18px 32px 14px;
    border-bottom: 1px solid #eae5da;
    gap: 8px;
    background: linear-gradient(to bottom, #fdfcf9, #fbf9f5);
  }

  .folio-top-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px;
    min-width: 0;
  }

  .folio-eyebrow {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: #7a7568;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }

  .eyebrow-dot {
    color: #b8b2a3;
  }

  .doc-title-input {
    border: none;
    background: transparent;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: clamp(1.1rem, 2.2vw, 1.55rem);
    font-weight: 700;
    color: #191f70;
    width: 100%;
    min-width: 0;
    outline: none;
  }

  @supports (font-size: 1cqi) {
    .doc-title-input {
      font-size: clamp(1.1rem, 3.2cqi, 1.55rem);
    }
  }

  .doc-sample-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .doc-chip-btn {
    border: 1px solid #dcd7ca;
    background: #f2efe7;
    color: #57534e;
    font-size: 12px;
    font-weight: 600;
    padding: 5px 11px;
    border-radius: 8px;
    cursor: pointer;
    transition: all 120ms ease;
  }

  .doc-chip-btn:hover {
    border-color: #191f70;
    background: #191f70;
    color: #ffffff;
  }

  .canvas-stage {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .canvas-backdrop,
  .canvas-textarea {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    padding: 26px 34px 68px;
    font-family: "Charter", "Iowan Old Style", "Palatino Linotype", Georgia, serif;
    font-size: 16.5px;
    line-height: 1.78;
    letter-spacing: 0.002em;
    white-space: pre-wrap;
    word-wrap: break-word;
    overflow-y: auto;
    border: none;
    margin: 0;
  }

  .canvas-backdrop {
    color: transparent;
    pointer-events: none;
    z-index: 1;
  }

  .canvas-textarea {
    background: transparent;
    color: #232220;
    caret-color: #191f70;
    resize: none;
    outline: none;
    z-index: 2;
  }

  .inline-lint-mark {
    background: color-mix(in oklab, var(--lint-color, #c84b31) 14%, transparent);
    color: transparent;
    border-bottom: 2px solid var(--lint-color, #c84b31);
    border-radius: 2px;
    padding-bottom: 1px;
  }

  .inline-lint-mark.selected {
    background: color-mix(in oklab, var(--lint-color, #c84b31) 26%, transparent);
  }

  .folio-colophon {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 28px;
    border-top: 1px solid #eae5da;
    background: #f6f3ec;
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 12px;
    color: #6b665b;
  }

  .format-tools {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .format-tools button {
    border: 1px solid transparent;
    background: transparent;
    color: #3f3d38;
    padding: 4px 8px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 12px;
  }

  .format-tools button:hover {
    background: #eae5da;
    border-color: #dcd7ca;
  }

  .canvas-metrics {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .tone-chip {
    background: color-mix(in oklab, #191f70 11%, #fbf9f5);
    color: #191f70;
    font-weight: 700;
    padding: 3px 9px;
    border-radius: 6px;
    border: 1px solid color-mix(in oklab, #191f70 22%, transparent);
  }

  /* 3. Right Unified "Proofreader's Margin" */
  .proofreader-margin {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: #f5f2ea;
    border: 1px solid #dfdbd0;
    border-radius: 18px;
    padding: 14px;
    min-height: 0;
    overflow: hidden;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.65);
  }

  .editorial-index-card {
    background: #fbf9f5;
    border: 1px solid #e2ddd2;
    border-radius: 14px;
    padding: 13px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow: 0 2px 8px rgba(24, 24, 27, 0.03);
  }

  .index-card-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }

  .index-eyebrow {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: #7a7568;
  }

  .index-score-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin-top: 2px;
  }

  .index-score-num {
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 30px;
    font-weight: 800;
    line-height: 1;
    font-variant-numeric: tabular-nums;
  }

  .index-status-pill {
    font-size: 11.5px;
    font-weight: 700;
    padding: 2px 9px;
    border-radius: 999px;
    color: var(--pill-accent, #191f70);
    background: color-mix(in oklab, var(--pill-accent, #191f70) 13%, #fbf9f5);
  }

  .goals-btn {
    border: 1px solid #dcd7ca;
    background: #ffffff;
    color: #191f70;
    font-size: 12px;
    font-weight: 600;
    padding: 6px 10px;
    border-radius: 8px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .goals-btn:hover {
    border-color: #191f70;
    background: #f5f2ea;
  }

  .goals-drawer {
    background: #f5f2ea;
    border: 1px solid #e2ddd2;
    border-radius: 10px;
    padding: 10px 12px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .goals-drawer h4 {
    grid-column: 1 / -1;
    margin: 0;
    font-size: 12px;
    color: #191f70;
  }

  .goal-field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 11px;
    color: #57534e;
    font-weight: 600;
  }

  .goal-field select {
    padding: 5px 7px;
    border-radius: 6px;
    border: 1px solid #d5cfc2;
    background: #fbf9f5;
    font-size: 12px;
    color: #18181b;
  }

  .dimension-grid {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 6px;
    padding-top: 8px;
    border-top: 1px solid #efeae0;
  }

  .dimension-tile {
    position: relative;
    border: 1px solid transparent;
    background: #f4f0e6;
    border-radius: 9px;
    padding: 6px 4px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    transition: all 120ms ease;
  }

  .dimension-tile:hover {
    border-color: #cfc9bc;
    background: #ebe6d9;
  }

  .dimension-tile.active {
    background: #191f70;
    border-color: #191f70;
  }

  .dimension-tile.active .dim-score,
  .dimension-tile.active .dim-name {
    color: #ffffff !important;
  }

  .dim-score {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 12.5px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }

  .dim-name {
    font-size: 11px;
    font-weight: 600;
    color: #6b665b;
  }

  .dim-count-dot {
    position: absolute;
    top: -4px;
    right: -3px;
    min-width: 15px;
    height: 15px;
    padding: 0 3px;
    border-radius: 999px;
    color: #ffffff;
    font-size: 9.5px;
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  /* Margin Annotations Header & Literary Cards */
  .margin-section-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 4px;
    gap: 8px;
  }

  .margin-section-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .margin-filter-reset {
    border: none;
    background: transparent;
    padding: 0;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    color: #57534e;
  }

  .margin-count-pill {
    background: #191f70;
    color: #ffffff;
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 11px;
  }

  .show-all-link {
    border: none;
    background: transparent;
    color: #191f70;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    text-decoration: underline;
  }

  .accept-all-btn {
    border: 1px solid rgba(25, 31, 112, 0.25);
    background: #fbf9f5;
    color: #191f70;
    font-size: 11.5px;
    font-weight: 700;
    padding: 4px 10px;
    border-radius: 7px;
    cursor: pointer;
  }

  .accept-all-btn:hover {
    background: #191f70;
    color: #ffffff;
  }

  .margin-notes-feed {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 9px;
    padding-right: 2px;
  }

  .alerts-empty-card {
    background: #fbf9f5;
    border-radius: 14px;
    padding: 24px 18px;
    text-align: center;
    border: 1px solid #e2ddd2;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }

  .alerts-empty-card h3 {
    margin: 4px 0 0;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 16px;
    color: #191f70;
  }

  .alerts-empty-card p {
    margin: 0;
    font-size: 12.5px;
    color: #6b665b;
    line-height: 1.45;
  }

  .margin-note-card {
    background: #fbf9f5;
    border: 1px solid #e2ddd2;
    border-left: 4px solid var(--card-accent, #c84b31);
    border-radius: 4px 12px 12px 4px;
    transition: all 120ms ease;
  }

  .margin-note-card.expanded {
    padding: 13px 14px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    box-shadow: 0 6px 18px rgba(24, 24, 27, 0.06);
  }

  .note-card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .note-eyebrow {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.04em;
  }

  .note-index-tag {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 11px;
    color: #8a8477;
  }

  .note-diff-box {
    background: #f4f0e6;
    border: 1px solid #e6e1d5;
    border-radius: 9px;
    padding: 8px 11px;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 7px;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 15px;
  }

  .feed-orig {
    color: #a1a1aa;
    text-decoration: line-through;
    text-decoration-color: #c84b31;
    font-weight: 500;
  }

  .feed-arrow {
    color: #8a8477;
    font-size: 13px;
  }

  .literary-replacement-btn {
    border: 1px solid rgba(25, 31, 112, 0.22);
    background: #ffffff;
    color: #191f70;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-weight: 700;
    font-size: 14.5px;
    padding: 2px 10px;
    border-radius: 6px;
    cursor: pointer;
    text-decoration: underline;
    text-decoration-color: #1e6f5c;
    text-decoration-thickness: 2px;
    text-underline-offset: 3px;
    transition: all 120ms ease;
  }

  .literary-replacement-btn:hover {
    background: #191f70;
    color: #ffffff;
    text-decoration-color: #ffffff;
  }

  .feed-card-explanation {
    margin: 0;
    font-size: 12.5px;
    color: #57534e;
    line-height: 1.46;
  }

  .feed-card-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-top: 6px;
    border-top: 1px solid #efeae0;
  }

  .feed-card-btns {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .feed-dict-btn,
  .note-text-action {
    border: 1px solid #e2ddd2;
    background: #f4f0e6;
    color: #57534e;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    padding: 4px 9px;
    border-radius: 6px;
  }

  .feed-dict-btn:hover,
  .note-text-action:hover {
    border-color: #191f70;
    color: #191f70;
  }

  .feed-card-icons {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .margin-note-card.compact {
    width: 100%;
    padding: 10px 12px;
    text-align: left;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .margin-note-card.compact:hover {
    border-color: #c5beaf;
    background: #ffffff;
  }

  .compact-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 10.5px;
    font-weight: 700;
  }

  .compact-diff-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 14px;
  }

  .compact-del {
    color: #8a8477;
  }

  .compact-repl {
    color: #191f70;
    font-weight: 700;
    text-decoration: underline;
    text-decoration-color: #1e6f5c;
  }

  /* 4. Pinned Bottom "Rewrite Atelier" Dock */
  .rewrite-atelier-dock {
    margin-top: auto;
    background: #fbf9f5;
    border: 1px solid #dcd7ca;
    border-radius: 14px;
    padding: 12px 13px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-shadow: 0 -2px 10px rgba(24, 24, 27, 0.03);
  }

  .atelier-dock-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .atelier-dock-title {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dock-spark {
    color: #191f70;
    font-size: 13px;
  }

  .atelier-dock-title h4 {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    color: #191f70;
  }

  .dock-scope-tag {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 10.5px;
    color: #7a7568;
  }

  .atelier-preset-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }

  .atelier-chip {
    border: 1px solid #dfd9cb;
    background: #f4f0e6;
    color: #191f70;
    font-size: 11.5px;
    font-weight: 600;
    padding: 4px 9px;
    border-radius: 999px;
    cursor: pointer;
    transition: all 120ms ease;
  }

  .atelier-chip:hover {
    background: #191f70;
    border-color: #191f70;
    color: #ffffff;
  }

  .ai-result-card {
    background: #f4f0e6;
    border: 1px solid #dfd9cb;
    border-radius: 9px;
    padding: 9px 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .ai-result-card p {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    color: #18181b;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 13.5px;
    line-height: 1.52;
    max-height: 150px;
    overflow-y: auto;
  }

  .ai-result-actions {
    display: flex;
    gap: 6px;
  }

  .ai-error-msg {
    margin: 0;
    font-size: 12px;
    color: #c84b31 !important;
    font-weight: 600;
  }

  @container (max-width: 340px) {
    .dimension-grid {
      grid-template-columns: repeat(3, 1fr);
    }
  }

  /* Settings Pages in Warm Literary Ink & Paper */
  .settings-page {
    flex: 1;
    overflow-y: auto;
    padding: 28px 34px;
    background: #efece4;
  }

  .settings-container {
    max-width: 880px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .settings-card {
    background: #fbf9f5;
    border: 1px solid #dfdbd0;
    border-radius: 16px;
    padding: 22px 26px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-shadow: 0 4px 14px rgba(24, 24, 27, 0.04);
  }

  .settings-card h2,
  .settings-card h3 {
    margin: 0;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    color: #191f70;
  }

  .settings-card p {
    margin: 0;
    font-size: 13.5px;
    color: #57534e;
    line-height: 1.52;
  }

  .settings-folio-title {
    font-size: 22px;
    margin-top: 3px !important;
    margin-bottom: 4px !important;
  }

  .ax-granted-inline {
    color: #1e6f5c;
    font-weight: 700;
  }

  .card-split-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }

  .ax-inline-strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    padding: 9px 13px;
    background: #f4f0e6;
    border: 1px solid #dfd9cb;
    border-left: 3px solid #191f70;
    border-radius: 9px;
    font-size: 12.5px;
    color: #3f3d38;
  }

  .ax-strip-left {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #3f3d38;
  }

  .ax-strip-left svg {
    color: #191f70;
    flex-shrink: 0;
  }

  .ax-strip-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .ax-strip-link {
    border: none;
    background: transparent;
    color: #6b665b;
    font-size: 12px;
    font-weight: 600;
    padding: 3px 6px;
    border-radius: 5px;
    cursor: pointer;
  }

  .ax-strip-link:hover {
    color: #18181b;
    background: rgba(24, 24, 27, 0.05);
  }

  .ax-strip-link.primary {
    color: #191f70;
    font-weight: 700;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .perm-badge {
    padding: 4px 10px;
    border-radius: 999px;
    font-size: 11.5px;
    font-weight: 700;
    background: rgba(30, 111, 92, 0.14);
    color: #1e6f5c;
  }

  .perm-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .settings-search {
    width: 100%;
    padding: 9px 12px;
    border: 1px solid #d5cfc2;
    border-radius: 9px;
    font-size: 13.5px;
    background: #ffffff;
    color: #18181b;
    outline: none;
  }

  .settings-search:focus {
    border-color: #191f70;
  }

  .app-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 8px;
    max-height: 440px;
    overflow-y: auto;
    padding-top: 2px;
  }

  .app-row-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 13px;
    background: #f5f2ea;
    border: 1px solid #e5e0d4;
    border-radius: 11px;
    cursor: pointer;
    gap: 10px;
    transition:
      background 120ms ease,
      border-color 120ms ease;
  }

  .app-row-card:hover {
    background: #ffffff;
    border-color: #cfc9bc;
  }

  .app-row-card.is-disabled {
    opacity: 0.68;
  }

  .app-row-left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    overflow: hidden;
  }

  .app-monogram {
    width: 28px;
    height: 28px;
    border-radius: 7px;
    background: color-mix(in oklab, #191f70 10%, #fbf9f5);
    border: 1px solid color-mix(in oklab, #191f70 18%, transparent);
    color: #191f70;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 13px;
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .app-row-info {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .app-row-info strong {
    font-size: 13.5px;
    font-weight: 600;
    color: #18181b;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .app-row-info span {
    font-size: 11px;
    color: #7a7568;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .toggle-switch-label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    font-weight: 600;
    color: #3f3d38;
    cursor: pointer;
    white-space: nowrap;
  }

  input[type="checkbox"][switch] {
    accent-color: #191f70;
    margin: 0;
    cursor: pointer;
    flex-shrink: 0;
  }

  input[type="checkbox"][switch]:focus-visible {
    outline: 2px solid #191f70;
    outline-offset: 2px;
  }

  .lexicon-folio-card {
    gap: 16px;
  }

  .lexicon-subblock {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 14px;
    border-top: 1px solid #ece7dc;
  }

  .lexicon-subblock.first-subblock {
    padding-top: 0;
    border-top: none;
  }

  .lexicon-subhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }

  .lexicon-subblock h4 {
    margin: 0;
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 15.5px;
    font-weight: 700;
    color: #191f70;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .lexicon-count-pill {
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
    font-size: 11px;
    font-weight: 600;
    color: #5c584e;
    background: #f2ede2;
    border: 1px solid #dfd9cb;
    padding: 2px 8px;
    border-radius: 999px;
  }

  .dialect-segmented-bar {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 6px;
    background: #f3efe5;
    padding: 5px;
    border-radius: 11px;
    border: 1px solid #e2ddd0;
  }

  .dialect-seg-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 8px 10px;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: #4a463e;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background 120ms ease,
      color 120ms ease,
      box-shadow 120ms ease;
  }

  .dialect-seg-btn:hover {
    color: #18181b;
    background: rgba(255, 255, 255, 0.55);
  }

  .dialect-seg-btn.active {
    background: #191f70;
    color: #ffffff;
    box-shadow: 0 2px 6px rgba(25, 31, 112, 0.22);
  }

  .dialect-code {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(24, 24, 27, 0.07);
    color: inherit;
  }

  .dialect-seg-btn.active .dialect-code {
    background: rgba(255, 255, 255, 0.18);
  }

  .dict-inline-form {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 280px;
    flex: 1;
    max-width: 360px;
  }

  .dict-inline-input {
    padding: 7px 11px;
    font-size: 12.5px;
  }

  .dict-add-btn {
    white-space: nowrap;
    padding: 7px 13px;
    font-size: 12.5px;
    flex-shrink: 0;
  }

  .dict-scroll-well {
    max-height: 112px;
    overflow-y: auto;
    padding: 8px 10px;
    background: #f6f3eb;
    border: 1px solid #e5e0d4;
    border-radius: 10px;
  }

  .dict-empty-hint {
    display: block;
    font-size: 12.5px;
    color: #7a7568;
    padding: 2px 2px;
  }

  .dict-chips-container {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .deferred-dict-chip {
    content-visibility: auto;
    contain-intrinsic-size: auto 64px auto 26px;
  }

  .dict-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
  }

  .dict-chip.custom-chip {
    background: color-mix(in oklab, #191f70 8%, #fbf9f5);
    border: 1px solid color-mix(in oklab, #191f70 22%, transparent);
    color: #191f70;
  }

  .dict-chip button {
    border: none;
    background: transparent;
    cursor: pointer;
    color: #191f70;
    opacity: 0.65;
    padding: 0 2px;
    font-size: 13px;
    line-height: 1;
  }

  .dict-chip button:hover {
    opacity: 1;
  }

  .rules-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .rules-toolbar-left {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  .rules-filter-pills {
    display: inline-flex;
    align-items: center;
    background: #f3efe5;
    border: 1px solid #e2ddd0;
    border-radius: 8px;
    padding: 2px;
    gap: 2px;
  }

  .rule-filter-pill {
    border: none;
    background: transparent;
    color: #5c584e;
    font-size: 11.5px;
    font-weight: 600;
    padding: 4px 10px;
    border-radius: 6px;
    cursor: pointer;
  }

  .rule-filter-pill.active {
    background: #ffffff;
    color: #191f70;
    box-shadow: 0 1px 2px rgba(24, 24, 27, 0.08);
  }

  .rules-search-input {
    max-width: 260px;
    padding: 7px 11px;
    font-size: 12.5px;
  }

  .rules-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 7px;
    max-height: 340px;
    overflow-y: auto;
    padding-top: 2px;
  }

  .rule-card-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 12px;
    background: #f5f2ea;
    border: 1px solid #e5e0d4;
    border-radius: 9px;
    cursor: pointer;
    transition:
      background 120ms ease,
      border-color 120ms ease;
  }

  .rule-card-row:hover {
    background: #ffffff;
    border-color: #cfc9bc;
  }

  .rule-card-row.is-disabled {
    opacity: 0.65;
  }

  .deferred-rule-row {
    content-visibility: auto;
    contain-intrinsic-size: auto none auto 40px;
  }

  .rule-card-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  .rule-card-info strong {
    font-size: 12.5px;
    font-weight: 600;
    color: #18181b;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .rule-card-info span {
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 10.5px;
    color: #7a7568;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .rules-empty-state {
    padding: 20px;
    text-align: center;
    font-size: 13px;
    color: #7a7568;
    background: #f7f4ec;
    border-radius: 9px;
    border: 1px dashed #dcd6c8;
  }

  .db-ledger-strip {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px;
    background: #f6f3eb;
    border: 1px solid #e4dfd2;
    border-radius: 11px;
  }

  .db-stat-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }

  .db-stat-tile {
    background: #ffffff;
    border: 1px solid #e5e0d4;
    border-radius: 8px;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .db-stat-num {
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 18px;
    font-weight: 700;
    color: #191f70;
    line-height: 1.1;
  }

  .db-stat-label {
    font-size: 11px;
    font-weight: 600;
    color: #6b665b;
  }

  .learned-edits-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-top: 4px;
  }

  .learned-edits-heading {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: #7a7568;
  }

  .learned-edits-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .learned-edit-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 9px;
    background: #ffffff;
    border: 1px solid #dfd9cb;
    border-radius: 999px;
    font-size: 11.5px;
    color: #18181b;
  }

  .learned-edit-pill .ev-cat {
    font-size: 10px;
    font-weight: 700;
    color: #1e6f5c;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .learned-edit-pill.is-dismissed .ev-cat {
    color: #b45309;
  }

  .voice-sample-box {
    width: 100%;
    padding: 12px;
    border: 1px solid #d5cfc2;
    border-radius: 9px;
    background: #ffffff;
    font-size: 13.5px;
    line-height: 1.5;
    resize: vertical;
  }

  .ai-form-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px 12px;
  }

  .form-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12.5px;
    font-weight: 600;
    color: #3f3d38;
  }

  .form-field.span-full {
    grid-column: 1 / -1;
  }

  .form-field label {
    display: block;
    cursor: pointer;
  }

  .form-field span {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }

  .field-hint {
    font-style: normal;
    font-size: 11px;
    font-weight: 500;
    color: #7a7568;
  }

  .field-hint.encrypted-ok {
    color: #1e6f5c;
    font-weight: 600;
  }

  .form-field input,
  .form-field select {
    padding: 8px 11px;
    border: 1px solid #d5cfc2;
    border-radius: 8px;
    font-size: 13px;
    background: #ffffff;
    color: #18181b;
    outline: none;
    width: 100%;
    box-sizing: border-box;
  }

  .form-field select {
    appearance: none;
    padding-right: 32px;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='14' height='14' viewBox='0 0 24 24' fill='none' stroke='%23191f70' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 10px center;
    cursor: pointer;
    font-weight: 500;
  }

  .form-field input:focus,
  .form-field select:focus {
    border-color: #191f70;
    box-shadow: 0 0 0 2px rgba(25, 31, 112, 0.1);
  }

  .form-field input:not([type="range"]):user-invalid {
    border-color: #c84b31;
  }

  .budget-progress-wrap {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 12px;
    background: #f6f3eb;
    border: 1px solid #e5e0d4;
    border-radius: 9px;
  }

  .budget-progress-labels {
    display: flex;
    justify-content: space-between;
    font-size: 11.5px;
    font-weight: 600;
    color: #4a463e;
  }

  .budget-progress-bar {
    width: 100%;
    height: 8px;
    accent-color: #191f70;
  }

  .elo-btn-secondary {
    border: 1px solid #cfc9bc;
    background: #f5f2ea;
    color: #18181b;
    padding: 8px 14px;
    border-radius: 8px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }

  .elo-btn-secondary:hover {
    background: #ffffff;
    border-color: #191f70;
  }

  .status-feedback {
    color: #1e6f5c !important;
    font-weight: 600;
    margin: 0;
    font-size: 12.5px;
  }

  .status-feedback.is-error {
    color: #c84b31 !important;
  }

  /* =======================================================================
     WRITING & AI ANALYTICS VIEW
     ======================================================================= */
  .analytics-kpi-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 10px;
  }

  .analytics-kpi-card {
    background: #f6f3eb;
    border: 1px solid #e4dfd2;
    border-radius: 10px;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .analytics-kpi-card.highlight-kpi {
    background: color-mix(in oklab, #191f70 6%, #fbf9f5);
    border-color: color-mix(in oklab, #191f70 24%, #e4dfd2);
  }

  .kpi-eyebrow {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: #7a7568;
  }

  .kpi-main-row {
    display: flex;
    align-items: baseline;
    gap: 4px;
  }

  .kpi-big-num {
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: clamp(1.25rem, 1rem + 0.9vw, 1.55rem);
    font-weight: 700;
    color: #191f70;
    line-height: 1.15;
  }

  .kpi-unit {
    font-size: 11.5px;
    font-weight: 600;
    color: #6b665b;
  }

  .kpi-sub {
    font-size: 11px;
    font-weight: 500;
    color: #5c584e;
  }

  .hybrid-split-bar {
    display: flex;
    height: 10px;
    width: 100%;
    border-radius: 999px;
    overflow: hidden;
    background: #e5e0d4;
    gap: 2px;
  }

  .hybrid-seg {
    height: 100%;
    transition: width 260ms ease;
  }

  .hybrid-seg.local-seg {
    background: #1e6f5c;
  }

  .hybrid-seg.ai-seg {
    background: #191f70;
  }

  .hybrid-cards-row {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }

  .hybrid-stat-box {
    background: #f6f3eb;
    border: 1px solid #e4dfd2;
    border-radius: 9px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .hybrid-box-head {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .hybrid-box-head strong {
    font-size: 12.5px;
    color: #18181b;
    flex: 1;
  }

  .hybrid-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .hybrid-dot.local-dot {
    background: #1e6f5c;
  }

  .hybrid-dot.ai-dot {
    background: #191f70;
  }

  .hybrid-pill {
    font-size: 10.5px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 999px;
    background: #ffffff;
    border: 1px solid #dfd9cb;
    color: #4a463e;
  }

  .hybrid-stat-box p {
    margin: 0;
    font-size: 11.5px;
    color: #5c584e;
  }

  .analytics-two-col {
    display: grid;
    grid-template-columns: 1.15fr 0.85fr;
    gap: 16px;
  }

  .analytics-col {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .pillar-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .pillar-row {
    background: #f6f3eb;
    border: 1px solid #e4dfd2;
    border-radius: 9px;
    padding: 8px 11px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .pillar-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .pillar-top strong {
    font-size: 12.5px;
    color: #18181b;
  }

  .pillar-score {
    font-size: 12px;
    font-weight: 700;
    color: #191f70;
  }

  .pillar-track {
    height: 6px;
    width: 100%;
    background: #e5e0d4;
    border-radius: 999px;
    overflow: hidden;
  }

  .pillar-fill {
    height: 100%;
    background: #191f70;
    border-radius: 999px;
  }

  .pillar-meta {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: #6b665b;
  }

  .cadence-chart {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    gap: 6px;
    background: #f6f3eb;
    border: 1px solid #e4dfd2;
    border-radius: 9px;
    padding: 10px 10px 8px;
    height: 128px;
    align-items: end;
  }

  .cadence-bar-col {
    display: flex;
    flex-direction: column;
    align-items: center;
    height: 100%;
    gap: 4px;
  }

  .cadence-val {
    font-size: 10px;
    font-weight: 700;
    color: #191f70;
    min-height: 13px;
  }

  .cadence-bar-well {
    flex: 1;
    width: 100%;
    max-width: 22px;
    background: #e8e3d6;
    border-radius: 5px;
    display: flex;
    align-items: flex-end;
    overflow: hidden;
  }

  .cadence-bar-fill {
    width: 100%;
    background: linear-gradient(180deg, #191f70 0%, #1e6f5c 100%);
    border-radius: 5px;
    transition: height 220ms ease;
  }

  .cadence-day {
    font-size: 10px;
    font-weight: 600;
    color: #6b665b;
    white-space: nowrap;
  }

  .manuscript-vitals-box {
    background: #f6f3eb;
    border: 1px solid #e4dfd2;
    border-radius: 9px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .vitals-mini-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }

  .vitals-mini-grid div {
    background: #ffffff;
    border: 1px solid #e5e0d4;
    border-radius: 7px;
    padding: 6px 8px;
    display: flex;
    flex-direction: column;
  }

  .vitals-mini-grid strong {
    font-size: 13px;
    font-weight: 700;
    color: #191f70;
  }

  .vitals-mini-grid span {
    font-size: 10.5px;
    color: #6b665b;
  }

  .analytics-table-wrapper {
    overflow: auto;
    max-inline-size: 100%;
    max-block-size: 220px;
    container-type: inline-size;
    border: 1px solid #e4dfd2;
    border-radius: 9px;
    background: #ffffff;
  }

  .analytics-model-table {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-size: 12px;
    --label-2: "AI Passes";
    --label-3: "Words Refined";
    --label-4: "Tokens";
    --label-5: "Spend";
  }

  .analytics-model-table thead {
    position: sticky;
    inset-block-start: 0;
    z-index: 1;
    background: #f6f3eb;
  }

  .analytics-model-table th,
  .analytics-model-table td {
    padding: 8px 11px;
    text-align: left;
    border-bottom: 1px solid #ede8dc;
  }

  .analytics-model-table thead th {
    font-size: 11px;
    font-weight: 700;
    color: #5c584e;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .analytics-model-table tbody th {
    position: sticky;
    inset-inline-start: 0;
    background: #ffffff;
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-weight: 600;
    color: #191f70;
  }

  @container (width < 520px) {
    .analytics-model-table,
    .analytics-model-table thead,
    .analytics-model-table tbody,
    .analytics-model-table tr,
    .analytics-model-table th,
    .analytics-model-table td {
      display: block;
    }

    .analytics-model-table thead {
      display: none;
    }

    .analytics-model-table td:nth-child(2)::before {
      content: var(--label-2) ": ";
      font-weight: 600;
    }

    .analytics-model-table td:nth-child(3)::before {
      content: var(--label-3) ": ";
      font-weight: 600;
    }

    .analytics-model-table td:nth-child(4)::before {
      content: var(--label-4) ": ";
      font-weight: 600;
    }

    .analytics-model-table td:nth-child(5)::before {
      content: var(--label-5) ": ";
      font-weight: 600;
    }
  }

  .feed-excerpt {
    font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
    font-size: 13px;
    line-height: 1.45;
    color: #27272a;
    background: rgba(37, 99, 235, 0.07);
    border-left: 2px solid #2563eb;
    padding: 4px 8px;
    border-radius: 4px;
    display: inline-block;
  }

  .compact-excerpt {
    font-size: 12px;
    color: #3f3d38;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  .ctx-highlight {
    background: rgba(37, 99, 235, 0.14);
    color: #1e3a8a;
    padding: 1px 4px;
    border-radius: 4px;
  }
</style>
