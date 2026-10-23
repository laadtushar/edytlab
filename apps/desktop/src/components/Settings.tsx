/**
 * Settings — API key + model selection UI.
 *
 * Two rendering modes:
 *  - `mode="blocking"`: full-screen modal that covers the chat until the
 *    user provides a key. Has no Close button (the only escape is to
 *    save a working key).
 *  - `mode="panel"`: opened from the gear icon in the header. Adds a
 *    Close button and a "Clear key" affordance.
 *
 * The model picker is a combo (text input + `<datalist>`) populated
 * from the Rust catalogue command for the active provider. The user
 * can type a free-form id (so brand-new models work the moment the
 * user knows their id), and the suggestion list shows entries pulled
 * from the live catalogue, or, for Anthropic without a key, a built-in
 * fallback list, and the hint under the field says which. Per-provider
 * selections are
 * persisted in localStorage under `edytlab.model.<provider>` so
 * switching provider preserves each side's choice.
 */

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import {
  clearApiKeyFor,
  installPlugin,
  listModelsFor,
  setApiKeyFor,
  setActiveModel,
  hasApiKeyFor,
  getActiveModel,
  getActiveProvider,
  setActiveProvider,
  getBaseUrlFor,
  defaultBaseUrlFor,
  setBaseUrlFor,
  getEffortFor,
  setEffortFor,
  testApiKeyFor,
  type KeychainPersistence,
  type ModelInfo,
  type ProviderId,
  type ReasoningEffort,
} from "../lib/tauri-bridge";
import { savedNotice } from "../lib/keychainNotice";
import {
  MODEL_STORAGE_KEY_PREFIX,
  modelStorageKey,
} from "../lib/model-storage";
import { AgentProfilesEditor } from "./AgentProfilesEditor";
import { McpServersEditor } from "./McpServersEditor";
import { MemoryEditor } from "./MemoryEditor";
import { MlRuntimeLine } from "./MlRuntimeLine";
import { SpeechModelRow } from "./SpeechModelRow";
import { ProjectEditor } from "./ProjectEditor";
import { SkillsEditor } from "./SkillsEditor";

/** Settings panel tabs. The blocking onboarding flow is locked to
 *  "account"; the panel mode lets the user switch. Phases 1-5 ship
 *  "account", "memory", "skills", "agents", "mcp", "plugins". */
type SettingsTab =
  | "account"
  | "project"
  | "memory"
  | "skills"
  | "agents"
  | "mcp"
  | "plugins";

export const LEGACY_MODEL_STORAGE_KEY = "edytlab.model";
export { MODEL_STORAGE_KEY_PREFIX };
export const PROVIDER_STORAGE_KEY = "edytlab.provider";

export const ANTHROPIC_KEYS_URL =
  "https://console.anthropic.com/settings/keys";
export const OPENROUTER_KEYS_URL = "https://openrouter.ai/keys";
export const OPENAI_KEYS_URL = "https://platform.openai.com/api-keys";
export const GROQ_KEYS_URL = "https://console.groq.com/keys";
export const GEMINI_KEYS_URL = "https://aistudio.google.com/apikey";
export const OLLAMA_DOCS_URL = "https://ollama.com/download";

// What the model field shows for a provider nothing was chosen for. Each is
// the backend's own default (`LlmProvider::default_model`, run through
// `translate_model`), and `crates/ai/tests/settings_default_models.rs`
// fails if the two drift.
const DEFAULT_MODEL_BY_PROVIDER: Record<ProviderId, string> = {
  anthropic: "claude-sonnet-5-5",
  openrouter: "anthropic/claude-sonnet-5.5",
  openai: "gpt-6-luna",
  groq: "llama-3.3-70b-versatile",
  gemini: "gemini-3.8-flash",
  ollama: "llama3.2",
};

const PROVIDERS: ReadonlyArray<{
  id: ProviderId;
  label: string;
  keyPlaceholder: string;
  keysUrl: string;
  /**
   * Providers that authenticate. A local daemon does not, and the
   * key field is hidden for it — mirrors `LlmProvider::requires_api_key`
   * on the Rust side, which is what actually gates saving and agent
   * construction.
   */
  needsKey?: boolean;
  /**
   * Providers with a reasoning-effort setting. Mirrors
   * `LlmProvider::supports_effort` on the Rust side: only Anthropic's
   * own API takes `output_config.effort`, and the backend refuses to
   * save a level for any other provider.
   */
  supportsEffort?: boolean;
}> = [
  {
    id: "anthropic",
    label: "Anthropic",
    keyPlaceholder: "sk-ant-...",
    keysUrl: ANTHROPIC_KEYS_URL,
    supportsEffort: true,
  },
  {
    id: "openrouter",
    label: "OpenRouter",
    keyPlaceholder: "sk-or-v1-...",
    keysUrl: OPENROUTER_KEYS_URL,
  },
  {
    id: "openai",
    label: "OpenAI",
    keyPlaceholder: "sk-...",
    keysUrl: OPENAI_KEYS_URL,
  },
  {
    id: "groq",
    label: "Groq",
    keyPlaceholder: "gsk_...",
    keysUrl: GROQ_KEYS_URL,
  },
  {
    id: "gemini",
    label: "Google Gemini",
    keyPlaceholder: "AIza...",
    keysUrl: GEMINI_KEYS_URL,
  },
  {
    id: "ollama",
    label: "Ollama (local)",
    keyPlaceholder: "",
    keysUrl: OLLAMA_DOCS_URL,
    needsKey: false,
  },
];

const DEFAULT_PROVIDER: ProviderId = "anthropic";

/**
 * The reasoning-effort choices, in the order the API ranks them. The
 * empty value is "Default": nothing is sent and the model uses its own.
 */
const EFFORT_OPTIONS: ReadonlyArray<{
  value: ReasoningEffort | "";
  label: string;
}> = [
  { value: "", label: "Default (model's own)" },
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "xhigh", label: "Extra high" },
  { value: "max", label: "Max" },
];

/**
 * How far from either end of the tab strip a tab has to sit to count as
 * in view: the width of the edge fade, so a tab that was scrolled into
 * reach is never left half dimmed under it (#392).
 */
const TAB_FADE_PX = 24;

function asEffort(value: string | null | undefined): ReasoningEffort | "" {
  return EFFORT_OPTIONS.some((o) => o.value === value && o.value !== "")
    ? (value as ReasoningEffort)
    : "";
}

function readStoredModel(provider: ProviderId): string {
  if (typeof window === "undefined") return DEFAULT_MODEL_BY_PROVIDER[provider];
  const ns = window.localStorage.getItem(modelStorageKey(provider));
  if (ns && ns.trim()) return ns;
  if (provider === "anthropic") {
    const legacy = window.localStorage.getItem(LEGACY_MODEL_STORAGE_KEY);
    if (legacy && legacy.trim()) {
      window.localStorage.setItem(modelStorageKey("anthropic"), legacy);
      window.localStorage.removeItem(LEGACY_MODEL_STORAGE_KEY);
      return legacy;
    }
  }
  return DEFAULT_MODEL_BY_PROVIDER[provider];
}

/**
 * Four outcomes, not two. "Reachable" and "can edit" are different
 * questions: tool support is a property of the model, and a local model
 * without it connects, tests green, and then fails on the first edit —
 * which is a confusing way to learn something we can say here.
 */
type TestState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "ok" }
  | { kind: "no-tools"; model: string; detail: string | null }
  | { kind: "model-missing"; model: string }
  | { kind: "err"; message: string };

export interface SettingsProps {
  mode: "blocking" | "panel";
  onSaved: () => void;
  onClose?: () => void;
  onCleared?: () => void;
  /**
   * Called when the active provider changes, with whether that provider
   * has a usable key (#250).
   *
   * Selecting a radio activates the provider immediately and rebuilds
   * the agent — which sets it to `None` when the new provider needs a
   * key and none is stored. The parent read `has_api_key` once on mount,
   * so nothing on screen reflected that the assistant had just been
   * turned off; the user found out at the next chat message.
   */
  onProviderChanged?: (hasKey: boolean) => void;
  /**
   * A note for the Account tab about where saved keys and settings go,
   * shown when they will not survive a reboot (#394). The same words as
   * the app's banner: in blocking mode this dialog covers the banner,
   * and it is exactly where a user whose key went with the last restart
   * is standing.
   */
  storageWarning?: string | null;
  /**
   * Open the Account tab scrolled so that this part of it is in view, for a
   * caller that sends the user there to press a button (#384). The speech
   * model's row is the last thing on a tall tab, so opening at the top left
   * it below the fold, and the speech-setup card says "press Download beside
   * it". Nothing is scrolled when this is absent, as for the gear.
   */
  reveal?: "speech-model" | null;
}

export function Settings({
  mode,
  onSaved,
  onClose,
  onCleared,
  onProviderChanged,
  storageWarning,
  reveal = null,
}: SettingsProps) {
  const [key, setKey] = useState("");
  /**
   * Whether the user has picked a provider since mount.
   *
   * A ref rather than state: nothing renders from it, and it has to be
   * readable by a promise callback that closed over its own render.
   */
  const pickedRef = useRef(false);
  const [provider, setProvider] = useState<ProviderId>(() => {
    if (typeof window === "undefined") return DEFAULT_PROVIDER;
    const stored = window.localStorage.getItem(PROVIDER_STORAGE_KEY);
    if (stored && PROVIDERS.some((p) => p.id === stored)) {
      return stored as ProviderId;
    }
    return DEFAULT_PROVIDER;
  });
  /**
   * Whether the selected provider authenticates. Mirrors
   * `LlmProvider::requires_api_key` on the Rust side, which is what
   * actually gates saving and agent construction — this only controls
   * what the form asks for.
   */
  const needsKey = PROVIDERS.find((p) => p.id === provider)?.needsKey !== false;
  const supportsEffort =
    PROVIDERS.find((p) => p.id === provider)?.supportsEffort === true;

  const [model, setModel] = useState<string>(() => readStoredModel(provider));
  // Whether the model field has focus. A model being typed is not a saved one
  // that has gone missing, so the "not in the list" warning waits for it.
  const [modelFocused, setModelFocused] = useState(false);
  // Empty means "use the provider's own endpoint". Kept per provider, so
  // switching to Anthropic does not inherit a local server's URL.
  const [baseUrl, setBaseUrl] = useState("");
  const [defaultBaseUrl, setDefaultBaseUrl] = useState("");
  // Empty means "Default": the model's own effort, nothing sent.
  const [effort, setEffort] = useState<ReasoningEffort | "">("");
  // Whether each field above holds something the user can mean: what was
  // stored, or what they typed over it. Empty is not that. It is also
  // what a field shows while its read is in flight and after a read the
  // keychain refused, and Save turns an empty URL into "delete the
  // override" and no level into "delete the level". Writing a field that
  // is not known erased the stored value of a user who had only dismissed
  // the startup prompts (#497).
  const [baseUrlKnown, setBaseUrlKnown] = useState(false);
  const [effortKnown, setEffortKnown] = useState(false);
  // The model field is the same case. It starts as what this browser
  // remembers or the provider's default, which is a thing to show, not a
  // thing that was read; the backend's answer is what makes it known.
  const [modelKnown, setModelKnown] = useState(false);
  // Why a read failed, per field, for the note that says Save leaves it be.
  const [baseUrlUnread, setBaseUrlUnread] = useState<string | null>(null);
  const [effortUnread, setEffortUnread] = useState<string | null>(null);
  const [modelUnread, setModelUnread] = useState<string | null>(null);
  const [test, setTest] = useState<TestState>({ kind: "idle" });
  const [saveError, setSaveError] = useState<string | null>(null);
  // Set when the last Save went through but some of it fell back to the
  // kernel keyring, so it lasts only until the next reboot (#497): the
  // launch banner's words for it. While it is set the form has done its
  // job and is waiting to be dismissed, so the dialog stays up for the
  // person to read it; closing it on `onSaved` would remove the note in
  // the same breath.
  const [savedSessionOnly, setSavedSessionOnly] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [models, setModels] = useState<ModelInfo[]>([]);
  // Which list `models` is: the provider's own, or edytlab's built-in one,
  // and why. Null until a fetch has answered, and after one has failed.
  const [catalogueSource, setCatalogueSource] = useState<
    "live" | "fallback" | null
  >(null);
  const [catalogueNotice, setCatalogueNotice] = useState<string | null>(null);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [tab, setTab] = useState<SettingsTab>("account");

  // The tab strip scrolls by itself (#392).
  //
  // Seven tabs only just fit in the 30rem card, and a narrower window or
  // a wider font tips them over. The card is `overflow-hidden` for its
  // rounded corners, which made *it* the nearest scroll container: focus
  // on a clipped tab (or anything calling `scrollIntoView`) slid the
  // whole dialog sideways and left Plugins half off it. So the strip is
  // the scroller, and `revealTab` moves only the strip, by hand, never
  // with `scrollIntoView`.
  //
  // The fade over each end that has tabs past it and the reveal share
  // `TAB_FADE_PX`: a tab brought into view is brought clear of the fade.
  //
  // A mouse press reveals on click, not on focus. In Chromium mousedown
  // focuses the button before mouseup, so a focus that scrolled the strip
  // would move a tab pressed inside the fade out from under the pointer.
  // The mouseup would land on the padding or the next tab, the click
  // would go to their common ancestor, and the tab would be focused but
  // not selected. Keyboard focus still reveals on focus.
  const tabsRef = useRef<HTMLDivElement>(null);
  const [tabEdges, setTabEdges] = useState({ start: false, end: false });

  const measureTabs = useCallback(() => {
    const strip = tabsRef.current;
    if (!strip) return;
    const max = strip.scrollWidth - strip.clientWidth;
    const start = strip.scrollLeft > 1;
    const end = strip.scrollLeft < max - 1;
    // Same answer, same object: scrolling must not re-render all of
    // Settings on every event.
    setTabEdges((prev) =>
      prev.start === start && prev.end === end ? prev : { start, end },
    );
  }, []);

  // A tab's width changes when the web font swaps in, so the strip and
  // each tab are watched, not just the window.
  useLayoutEffect(() => {
    const strip = tabsRef.current;
    if (mode !== "panel" || !strip) return;
    measureTabs();
    if (typeof ResizeObserver === "function") {
      const observer = new ResizeObserver(measureTabs);
      observer.observe(strip);
      strip
        .querySelectorAll('[role="tab"]')
        .forEach((el) => observer.observe(el));
      return () => observer.disconnect();
    }
    window.addEventListener("resize", measureTabs);
    return () => window.removeEventListener("resize", measureTabs);
  }, [mode, measureTabs]);

  const revealTab = useCallback(
    (tabEl: HTMLElement) => {
      const strip = tabsRef.current;
      if (!strip) return;
      const max = strip.scrollWidth - strip.clientWidth;
      // Everything fits: nothing to do.
      if (max <= 0) return;
      const s = strip.getBoundingClientRect();
      const t = tabEl.getBoundingClientRect();
      const delta =
        t.left < s.left + TAB_FADE_PX
          ? t.left - (s.left + TAB_FADE_PX)
          : t.right > s.right - TAB_FADE_PX
            ? t.right - (s.right - TAB_FADE_PX)
            : 0;
      if (delta === 0) return;
      // Clamped here: a browser clamps the assignment itself, but jsdom's
      // stub does not, and the flags are computed from the result.
      strip.scrollLeft = Math.min(max, Math.max(0, strip.scrollLeft + delta));
      measureTabs();
    },
    [measureTabs],
  );

  const tabsMask =
    tabEdges.start || tabEdges.end
      ? `linear-gradient(to right, ${tabEdges.start ? "transparent" : "#000"}, #000 ${TAB_FADE_PX}px, #000 calc(100% - ${TAB_FADE_PX}px), ${tabEdges.end ? "transparent" : "#000"})`
      : null;

  // The body scrolls (see the card), and it is one element for every tab, so
  // a tab opens at its top: the scroll left over from a tall tab would start
  // the next one half way down it.
  const bodyRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const body = bodyRef.current;
    if (body) body.scrollTop = 0;
  }, [tab]);

  // Bring the row a caller asked for to near the top of the body (`reveal`).
  //
  // Only the body is scrolled, by hand: `scrollIntoView` would also scroll
  // the card's `overflow-hidden` ancestors (see the tab strip above). The
  // fields above the row finish loading after the tab opens (the model list,
  // the effort level, the runtime line), which moves the row down, so the
  // placement is repeated when the content resizes, until the user takes the
  // scroll over (wheel, press or key) or a few seconds have passed.
  useEffect(() => {
    if (reveal !== "speech-model" || mode !== "panel" || tab !== "account") return;
    const body = bodyRef.current;
    if (!body) return;
    let done = false;
    const place = () => {
      if (done) return;
      const row = body.querySelector("[data-testid='settings-speech-model']");
      if (!row) return;
      const delta = row.getBoundingClientRect().top - body.getBoundingClientRect().top - 12;
      if (delta !== 0) body.scrollTop += delta;
    };
    place();
    const content = body.firstElementChild;
    const observer =
      typeof ResizeObserver !== "undefined" && content ? new ResizeObserver(place) : null;
    if (content) observer?.observe(content);
    const stop = () => {
      done = true;
      observer?.disconnect();
    };
    const events = ["wheel", "pointerdown", "keydown", "touchstart"] as const;
    events.forEach((e) => body.addEventListener(e, stop, { passive: true }));
    const timer = window.setTimeout(stop, 4000);
    return () => {
      stop();
      window.clearTimeout(timer);
      events.forEach((e) => body.removeEventListener(e, stop));
    };
  }, [reveal, mode, tab]);

  const [pluginSource, setPluginSource] = useState('');
  const [pluginInstalling, setPluginInstalling] = useState(false);
  const [pluginResult, setPluginResult] = useState<string | null>(null);

  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(PROVIDER_STORAGE_KEY, provider);
    }
  }, [provider]);

  /**
   * The *agent's* provider wins over what this browser profile
   * remembers (#225 §3).
   *
   * Exactly the problem #249 fixed for the model, left unfixed for the
   * provider one line above it: the radio group read `localStorage`
   * alone, nothing ever called `get_active_provider`, and the two had
   * no way to notice they disagreed. When they did, Settings showed
   * one provider while the agent answered on another — and every
   * keyed-off-provider action in this panel (the model list, the base
   * URL, Test, Clear) operated on the one on screen.
   *
   * They diverge for real reasons, not just theory: a `set_active_provider`
   * that rejects leaves the UI advanced and the backend behind, and a
   * profile whose localStorage is cleared falls back to the default
   * while the keychain still holds another choice.
   *
   * localStorage stays the immediate value so the control is never
   * blank while the IPC is in flight — same trade as the model, for
   * the same reason.
   */
  useEffect(() => {
    let cancelled = false;
    void getActiveProvider()
      .then((active) => {
        if (cancelled) return;
        // The user got there first. `cancelled` only covers unmount,
        // so without this a click landing while the read is in flight
        // is undone by it: `handleProviderChange` has already advanced
        // the UI and told the backend, and this would snap the radio
        // back to the value that read before either happened. Raised
        // in review on #324.
        //
        // Hydration is a starting point, not a correction. Once there
        // is a selection to correct, it has missed its moment.
        if (pickedRef.current) return;
        // An id this build does not offer is not selectable, and
        // showing it would leave the radio group with nothing lit.
        if (!PROVIDERS.some((p) => p.id === active)) return;
        setProvider(active);
      })
      .catch(() => {
        // The stored value is still a reasonable thing to show, and
        // picking any provider reconciles both sides.
      });
    return () => {
      cancelled = true;
    };
    // Mount only. After this, the radio group is the authority and it
    // writes through `setActiveProvider`; re-running on every change
    // would fight the user's own selection.
  }, []);

  // What the *agent* is configured with wins over what this browser
  // profile remembers (#249).
  //
  // The dropdown used to read localStorage alone, and nothing ever
  // consulted `get_active_model` — so the two could disagree with no
  // way to notice. They disagreed on every restart, because the model
  // lived only in memory on the Rust side.
  //
  // localStorage stays the immediate value so the control is never
  // blank while the IPC is in flight, and remains the answer when the
  // backend has no opinion yet (nothing chosen for this provider).
  //
  // An answer, empty or not, is what makes the field known (see
  // `modelKnown`). A rejection is not one: after a startup the keychain
  // refused, the chosen model was never restored, and the backend says so
  // instead of answering "none chosen" (#497). The stored value stays on
  // screen then, and Save leaves the model alone.
  useEffect(() => {
    setModel(readStoredModel(provider));
    // Another provider's model is not this one's, and until this one's
    // read answers there is nothing here to save.
    setModelKnown(false);
    setModelUnread(null);

    let cancelled = false;
    void getActiveModel(provider)
      .then((active) => {
        if (cancelled) return;
        setModelKnown(true);
        if (!active.trim()) return;
        setModel(active);
        if (typeof window !== "undefined") {
          window.localStorage.setItem(modelStorageKey(provider), active);
        }
      })
      .catch((err) => {
        // No error banner: the stored value is still a reasonable thing
        // to show, and a note under the field says why Save skips it.
        if (!cancelled) setModelUnread(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [provider]);

  useEffect(() => {
    let cancelled = false;
    // Another provider's URL is not this one's, and until this one's read
    // answers there is nothing here to save.
    setBaseUrl("");
    setBaseUrlKnown(false);
    setBaseUrlUnread(null);
    // Asked apart: the placeholder is only a hint, and a read of the
    // stored URL that failed must not take it down, nor the reverse. A
    // settings panel that cannot read one optional field should still
    // open; the placeholder simply stays blank.
    void (async () => {
      try {
        const fallback = await defaultBaseUrlFor(provider);
        if (!cancelled) setDefaultBaseUrl(fallback);
      } catch {
        // The placeholder stays blank.
      }
    })();
    void (async () => {
      try {
        const stored = await getBaseUrlFor(provider);
        if (cancelled) return;
        setBaseUrl(stored ?? "");
        setBaseUrlKnown(true);
      } catch (err) {
        // The field stays blank and unknown, so Save leaves the stored
        // URL alone.
        if (!cancelled) setBaseUrlUnread(String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [provider]);

  // The stored reasoning effort, for a provider that has one. Cleared
  // first so a level read for Anthropic is not left on screen if the
  // user switches away and back before the read answers.
  useEffect(() => {
    setEffort("");
    setEffortKnown(false);
    setEffortUnread(null);
    if (!supportsEffort) return;
    let cancelled = false;
    void (async () => {
      try {
        const stored = await getEffortFor(provider);
        if (cancelled) return;
        setEffort(asEffort(stored));
        setEffortKnown(true);
      } catch (err) {
        // Optional setting: the panel opens on "Default" without it, and
        // Save leaves the stored level alone.
        if (!cancelled) setEffortUnread(String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [provider, supportsEffort]);

  const fetchCatalogue = useCallback(
    async (p: ProviderId, k: string | undefined) => {
      try {
        const catalogue = await listModelsFor(p, k);
        setModels(catalogue.models);
        setCatalogueSource(catalogue.source);
        setCatalogueNotice(catalogue.notice);
        setModelsError(null);
      } catch (err) {
        setModels([]);
        setCatalogueSource(null);
        setCatalogueNotice(null);
        setModelsError(String(err));
      }
    },
    [],
  );

  useEffect(() => {
    void fetchCatalogue(provider, key.trim() || undefined);
  }, [provider, fetchCatalogue]);

  useEffect(() => {
    if (!key.trim()) return;
    const t = window.setTimeout(() => {
      void fetchCatalogue(provider, key.trim());
    }, 400);
    return () => window.clearTimeout(t);
  }, [key, provider, fetchCatalogue]);

  const handleProviderChange = useCallback(
    async (next: ProviderId) => {
      if (next === provider) return;
      // Before any await: the in-flight hydration must see this even
      // if it resolves during the calls below.
      pickedRef.current = true;
      setProvider(next);
      setKey("");
      setTest({ kind: "idle" });
      setSaveError(null);
      // Another provider is another form; the note was about the last Save.
      setSavedSessionOnly(null);
      try {
        await setActiveProvider(next);

        // Activation is immediate and deliberate — you can select a
        // provider before you have a key for it. What was missing is
        // that this *also* deactivates a working agent when the new
        // provider has no key, and nothing said so (#250). The user
        // clicked a radio to look at a model list and found out at the
        // next chat message.
        //
        // `has_api_key_for` was written for exactly this and had no
        // caller.
        const providerNeedsKey =
          PROVIDERS.find((p) => p.id === next)?.needsKey !== false;
        const hasKey = await hasApiKeyFor(next);
        onProviderChanged?.(hasKey || !providerNeedsKey);
        if (providerNeedsKey && !hasKey) {
          setSaveError(
            `Switched to ${next}, which has no API key stored — the ` +
              `assistant is inactive until you save one here.`,
          );
        }
      } catch (err) {
        setSaveError(String(err));
      }
    },
    [provider, onProviderChanged],
  );

  const handleModelChange = useCallback(
    (next: string) => {
      setModel(next);
      // Typed over: the user means this, whatever was stored.
      setModelKnown(true);
      if (typeof window !== "undefined") {
        window.localStorage.setItem(modelStorageKey(provider), next);
      }
      // Reported, not swallowed (#249). This call is what actually
      // points the agent at the model; localStorage above only decides
      // what this dropdown draws. If it fails, the two disagree — the
      // user reads their choice on screen while the agent runs
      // something else — and that is exactly the state worth saying out
      // loud.
      setSaveError(null);
      void setActiveModel(provider, next).catch((err) => {
        setSaveError(`could not select ${next}: ${String(err)}`);
      });
    },
    [provider],
  );

  const handleSave = useCallback(async () => {
    if ((needsKey && !key.trim()) || saving) return;
    setSaving(true);
    setSaveError(null);
    setSavedSessionOnly(null);
    // Where each write landed. A Save is several: a field that fell back
    // to the kernel keyring is lost at the next reboot whichever one it is.
    const landings: Array<KeychainPersistence | null | undefined> = [];
    try {
      // Saved first: `set_api_key_for` rebuilds the agent, and the
      // rebuild reads this. The other order builds an agent pointing at
      // the old endpoint until something else triggers another rebuild.
      //
      // Only a URL this form knows (see `baseUrlKnown`): an empty field
      // that stands for "not read" would be sent as a delete.
      if (baseUrlKnown) {
        landings.push(await setBaseUrlFor(provider, baseUrl));
      }
      // Same reason, same place: the rebuild reads the stored effort.
      // "Default" is sent as null, which clears the slot. Same condition
      // too.
      if (supportsEffort && effortKnown) {
        landings.push(await setEffortFor(provider, effort === "" ? null : effort));
      }
      // A keyless provider is activated, not written.
      //
      // This used to call `setApiKeyFor(provider, "")` for every
      // provider, on the reasoning that an empty string is how the UI
      // says "make this one active". The keychain does not accept an
      // empty secret — it answers `Attribute secret is invalid: cannot
      // be empty` — so with Ollama selected, Save failed.
      //
      // It hid behind the first-run path. Clicking the Ollama radio
      // calls `setActiveProvider` and reports the provider usable, and
      // that is what dismisses the blocking modal, so a *new* user
      // never noticed Save had also errored. A returning user opens
      // the app with Ollama already restored from storage, the radio
      // handler never fires because the selection has not changed, and
      // the only button on a modal with no Close is one that cannot
      // succeed. Found on the second launch of the real app.
      if (needsKey) {
        landings.push(await setApiKeyFor(provider, key));
      } else {
        landings.push(await setActiveProvider(provider));
      }
      // Only a model this form knows (see `modelKnown`): the field of a
      // read that was refused is the browser's memory or the default.
      if (modelKnown && model.trim()) {
        try {
          landings.push(await setActiveModel(provider, model));
        } catch {
          /* non-fatal */
        }
      }
      setKey("");
      setTest({ kind: "idle" });
      // A durable save shows nothing new. One that fell back stays on
      // screen until the person goes on (`handleContinue`).
      const notice = savedNotice(landings);
      if (notice) {
        setSavedSessionOnly(notice.message);
      } else {
        onSaved();
      }
    } catch (err) {
      setSaveError(String(err));
    } finally {
      setSaving(false);
    }
  }, [
    key,
    needsKey,
    provider,
    model,
    modelKnown,
    baseUrl,
    baseUrlKnown,
    effort,
    effortKnown,
    supportsEffort,
    saving,
    onSaved,
  ]);

  const handleTest = useCallback(async () => {
    // Testing a keyless provider is a reachability check, so an empty
    // key is the expected input rather than a reason to bail.
    if (needsKey && !key.trim()) return;
    setTest({ kind: "running" });
    try {
      // The typed values, not the saved ones — testing settings before
      // committing them is what the button is for.
      const report = await testApiKeyFor(
        provider,
        key,
        baseUrl.trim() || undefined,
        model.trim() || undefined,
      );
      setTest(
        report.modelUnavailable
          ? { kind: "model-missing", model: report.model }
          : report.toolsOk
          ? { kind: "ok" }
          : { kind: "no-tools", model: report.model, detail: report.detail },
      );
    } catch (err) {
      setTest({ kind: "err", message: String(err) });
    }
  }, [key, needsKey, provider, baseUrl, model]);

  /**
   * Remove the key for the provider that is on screen.
   *
   * Saving is explicit — `setApiKeyFor(provider, key)` — but clearing
   * went through `clearApiKey()`, which takes no argument and resolves
   * the slot from the *backend's* active provider. The two agree in
   * the normal flow, because picking a provider awaits
   * `setActiveProvider` before anything else can happen, so this was
   * not the silent wrong-credential delete it looks like.
   *
   * It is still the wrong call. It makes deleting a secret depend on
   * the UI and the backend agreeing about which provider is current,
   * and #225 §3 is open precisely because they have two sources of
   * truth for that. The one path where they diverge is real: if
   * `setActiveProvider` rejects, `provider` has already advanced to
   * the new selection while the backend still holds the old one, and
   * Clear then removes a key the user is not looking at.
   *
   * `clearApiKeyFor` says which provider it means, so it cannot drift.
   * Named the same way as its save counterpart, for a destructive
   * action on a credential, where being explicit is worth more.
   */
  const handleClear = useCallback(async () => {
    try {
      await clearApiKeyFor(provider);
      setKey("");
      setTest({ kind: "idle" });
      onCleared?.();
    } catch (err) {
      setSaveError(String(err));
    }
  }, [onCleared, provider]);

  // Going on after a Save that fell back: the save happened, and this is the
  // `onSaved` that Save held back until the note had been read.
  const handleContinue = useCallback(() => {
    setSavedSessionOnly(null);
    onSaved();
  }, [onSaved]);

  const handleInstallPlugin = useCallback(async () => {
    if (!pluginSource.trim()) return;
    setPluginInstalling(true);
    setPluginResult(null);
    try {
      const result = await installPlugin(pluginSource.trim());
      setPluginResult(result?.summary ?? 'Plugin installed.');
    } catch (e) {
      setPluginResult(`Error: ${String(e)}`);
    } finally {
      setPluginInstalling(false);
    }
  }, [pluginSource]);

  const activeProviderEntry =
    PROVIDERS.find((p) => p.id === provider) ?? PROVIDERS[0];

  const saveDisabled = (needsKey && !key.trim()) || saving;
  const testDisabled = (needsKey && !key.trim()) || test.kind === "running";

  // The scrim fades in like the other overlays' (`backdrop-in`), and has
  // no `backdrop-blur`: animating the opacity of a layer that blurs what
  // is behind it makes WebKitGTK re-blur the whole window every frame.
  // The darker fill does the blur's job of pushing the app back.
  const containerClass =
    mode === "blocking"
      ? "fixed inset-0 z-50 flex items-center justify-center bg-[radial-gradient(ellipse_at_center,rgba(0,0,0,0.85),rgba(0,0,0,0.97))] backdrop-in"
      : "fixed inset-0 z-40 flex items-center justify-center bg-black/60 backdrop-in";

  const suggestions = models.map((m) => {
    const label =
      m.display_name && m.display_name !== m.id
        ? `${m.id} — ${m.display_name}`
        : m.id;
    return { id: m.id, label };
  });

  const providerLabel = activeProviderEntry.label;
  // In order: the fetch failed; the list is edytlab's own and not the
  // provider's (the person should know that before trusting it as complete);
  // the provider's list.
  const modelHintKind: "error" | "fallback" | "live" | null =
    modelsError !== null
      ? "error"
      : catalogueSource === "fallback"
      ? "fallback"
      : models.length > 0
      ? "live"
      : null;
  const modelHint =
    modelHintKind === "error"
      ? `Couldn't fetch model list (${modelsError}); type the model id manually.`
      : modelHintKind === "fallback"
      ? `Showing edytlab's built-in list${
          catalogueNotice ? ` (${catalogueNotice})` : ""
        }. Save your key to load ${providerLabel}'s current models, or type any model id.`
      : modelHintKind === "live"
      ? `${models.length} model${models.length === 1 ? "" : "s"} available — start typing for suggestions.`
      : "";

  // The saved model is missing from Anthropic's own current list: it may have
  // been retired, and the first message on it would fail. Only for a list the
  // provider answered with (the built-in one is edytlab's, and lacking a model
  // says nothing), and only for Anthropic, whose list is complete; another
  // provider's is filtered (OpenAI) or spells ids another way (OpenRouter), so
  // absence proves nothing there. A pre-4.6 alias such as `claude-haiku-4-5`
  // resolves to the dated id the list holds, so `<id>-<8 digits>` counts as
  // listed (<https://platform.claude.com/docs/en/about-claude/models/overview>).
  const savedModel = model.trim();
  const modelUnlisted =
    provider === "anthropic" &&
    catalogueSource === "live" &&
    savedModel !== "" &&
    !modelFocused &&
    !models.some(
      (m) => m.id === savedModel || m.id.match(/^(.+)-\d{8}$/)?.[1] === savedModel,
    );

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      data-testid="settings"
      data-mode={mode}
      className={containerClass}
    >
      {/*
        The card is a column that never outgrows the window: it is capped at
        the window's height less 2rem, which centring turns into 1rem of
        margin above and below. The header and the tab strip keep their
        height, the body takes what is left and scrolls, and the footer
        (Save, on the Account tab) is outside the scroll, so it is on screen
        whatever is in the body. `vh`, not `max-h-full`: the container is
        the window, and a viewport unit needs no height to be definite.

        It was a block with `overflow-hidden` and no height cap, centred in
        a `fixed inset-0` box. Once the storage warning (#477) and the
        model-list note (#504) joined the Account tab it was 874 px tall in
        a 775 px window, ran off both edges, and had nothing to scroll: a
        new user could not reach Save & Continue.
      */}
      <div
        data-testid="settings-card"
        className="
          overlay-in
          relative flex max-h-[calc(100vh-2rem)] w-[30rem] max-w-[90vw] flex-col overflow-hidden
          rounded-xl border border-[var(--border-strong)]
          bg-[var(--surface-elev)]
          shadow-(--shadow-dialog)
        "
      >
        <div className="flex shrink-0 items-center justify-between border-b border-[var(--border)] px-5 py-4">
          <div className="flex items-baseline gap-2">
            <h2 className="text-lg font-semibold leading-none tracking-tight text-[var(--text)]">
              {mode === "blocking" ? (
                <>
                  <span className="gradient-text">Welcome</span>{" "}
                  to edytlab
                </>
              ) : (
                "Settings"
              )}
            </h2>
          </div>
          {mode === "panel" && onClose ? (
            <button
              type="button"
              // After a Save that fell back, Close is Continue: the save
              // happened, and the app has to hear of it.
              onClick={savedSessionOnly !== null ? handleContinue : onClose}
              data-testid="settings-close"
              aria-label="Close settings"
              className="btn-ghost rounded-md p-1.5 transition"
            >
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
                <path d="M3 3l8 8M11 3l-8 8" />
              </svg>
            </button>
          ) : null}
        </div>

        {mode === "panel" ? (
          // The divider is on this wrapper, not on the strip: the fade is a
          // mask on the strip, and would fade the divider line with it.
          <div className="shrink-0 border-b border-[var(--border)]">
            <div
              ref={tabsRef}
              data-testid="settings-tabs"
              role="tablist"
              data-overflow-start={tabEdges.start}
              data-overflow-end={tabEdges.end}
              onScroll={measureTabs}
              style={
                tabsMask
                  ? { maskImage: tabsMask, WebkitMaskImage: tabsMask }
                  : undefined
              }
              // `hidden`, not a zero height: styles.css sizes every
              // `::-webkit-scrollbar` outside the cascade layers, which
              // beats a utility on height but sets no `display`.
              className="flex items-center overflow-x-auto overflow-y-hidden px-3 pt-3 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
            >
              <SettingsTabButton
                id="account"
                label="Account"
                active={tab === "account"}
                onClick={() => setTab("account")}
                onReveal={revealTab}
              />
              <SettingsTabButton
                id="project"
                label="Project"
                active={tab === "project"}
                onClick={() => setTab("project")}
                onReveal={revealTab}
              />
              <SettingsTabButton
                id="memory"
                label="Memory"
                active={tab === "memory"}
                onClick={() => setTab("memory")}
                onReveal={revealTab}
              />
              <SettingsTabButton
                id="skills"
                label="Skills"
                active={tab === "skills"}
                onClick={() => setTab("skills")}
                onReveal={revealTab}
              />
              <SettingsTabButton
                id="agents"
                label="Agents"
                active={tab === "agents"}
                onClick={() => setTab("agents")}
                onReveal={revealTab}
              />
              <SettingsTabButton
                id="mcp"
                label="MCP"
                active={tab === "mcp"}
                onClick={() => setTab("mcp")}
                onReveal={revealTab}
              />
              <SettingsTabButton
                id="plugins"
                label="Plugins"
                active={tab === "plugins"}
                onClick={() => setTab("plugins")}
                onReveal={revealTab}
              />
            </div>
          </div>
        ) : null}

        <div
          ref={bodyRef}
          data-testid="settings-body"
          // No bottom padding on Account: its last field ends in a `mb-4` of
          // its own, which is the gap above the footer's divider.
          className={
            "min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-5 pt-4 " +
            (tab === "account" ? "pb-0" : "pb-4")
          }
        >
          {/* One tab's content, replaced whole when the tab changes: the
              key remounts it and it fades in over --dur-1. Opacity only,
              so no box moves (`settings-tabs.spec` measures them), and the
              editors already remount per tab. */}
          <div key={tab} className="fade-in-quick">
          {tab === "project" && mode === "panel" ? <ProjectEditor /> : null}
          {tab === "memory" && mode === "panel" ? <MemoryEditor /> : null}
          {tab === "skills" && mode === "panel" ? <SkillsEditor /> : null}
          {tab === "agents" && mode === "panel" ? (
            <AgentProfilesEditor />
          ) : null}
          {tab === "mcp" && mode === "panel" ? <McpServersEditor /> : null}
          {tab === "plugins" && mode === "panel" ? (
            <div data-testid="plugins-panel">
              <p className="mb-1.5 font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
                Install Plugin
              </p>
              <p className="mb-3 text-sm leading-relaxed text-[var(--text-dim)]">
                Source format:{" "}
                <code className="rounded bg-[var(--surface)] px-1 py-0.5 font-mono text-xs text-[var(--text)]">
                  github:org/repo
                </code>{" "}
                or{" "}
                <code className="rounded bg-[var(--surface)] px-1 py-0.5 font-mono text-xs text-[var(--text)]">
                  local:/path/to/dir
                </code>
              </p>
              <div className="mb-3 flex gap-2">
                <input
                  data-testid="plugin-source-input"
                  placeholder="github:edytlab-community/podcast-toolkit"
                  value={pluginSource}
                  onChange={e => setPluginSource(e.target.value)}
                  onKeyDown={e => e.key === 'Enter' && void handleInstallPlugin()}
                  className="
                    field
                    w-full
                    px-3 py-2 text-sm font-mono
                    outline-none
                    transition
                    placeholder:font-sans
                  "
                />
                <button
                  type="button"
                  data-testid="plugin-install-btn"
                  onClick={() => void handleInstallPlugin()}
                  disabled={pluginInstalling || !pluginSource.trim()}
                  className="btn-primary whitespace-nowrap px-4 py-1.5 text-sm transition"
                >
                  {pluginInstalling ? 'Installing…' : 'Install'}
                </button>
              </div>
              {pluginResult && (
                <p
                  data-testid="plugin-result"
                  className={
                    "rounded-md border px-3 py-1.5 text-xs " +
                    (pluginResult.startsWith('Error:')
                      ? "border-[var(--danger)]/40 bg-[var(--danger)]/10 text-[var(--danger)]"
                      : "border-[var(--success)]/35 bg-[var(--success)]/10 text-[var(--success)]")
                  }
                >
                  {pluginResult}
                </p>
              )}
            </div>
          ) : null}
          {tab === "account" ? (
          <>
          {mode === "blocking" ? (
            <p className="mb-5 text-sm leading-relaxed text-[var(--text-dim)]">
              edytlab needs an LLM API key to power the assistant. Pick a
              provider below; your key is stored in your OS keychain —
              never on disk in plaintext.
            </p>
          ) : null}
          {storageWarning ? (
            <p
              data-testid="settings-storage-warning"
              role="note"
              className="mb-4 rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-1.5 text-xs text-[var(--warning)]"
            >
              {storageWarning}
            </p>
          ) : null}

          <fieldset
            className="mb-4"
            data-testid="settings-provider-picker"
            aria-label="LLM provider"
          >
            <legend className="mb-1.5 block font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
              Provider
            </legend>
            <div className="grid grid-cols-3 gap-1.5">
              {PROVIDERS.map((p) => (
                <label
                  key={p.id}
                  className={
                    "flex cursor-pointer items-center justify-center gap-2 rounded-md border px-2 py-2 text-sm transition " +
                    (provider === p.id
                      ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-text)]"
                      : "border-[var(--border-strong)] text-[var(--text-dim)] hover:border-[var(--text-faint)] hover:text-[var(--text)]")
                  }
                >
                  <input
                    type="radio"
                    name="provider"
                    value={p.id}
                    checked={provider === p.id}
                    onChange={() => {
                      void handleProviderChange(p.id);
                    }}
                    data-testid={`settings-provider-${p.id}`}
                    className="sr-only"
                  />
                  {p.label}
                </label>
              ))}
            </div>
          </fieldset>

          {needsKey ? (
            <>
          <label className="mb-1.5 block font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            {activeProviderEntry.label} API key
          </label>
          <input
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={key}
            onChange={(e) => {
              setKey(e.target.value);
              if (test.kind !== "idle") setTest({ kind: "idle" });
            }}
            placeholder={activeProviderEntry.keyPlaceholder}
            data-testid="settings-key-input"
            aria-label={`${activeProviderEntry.label} API key`}
            className="
              field
              mb-2 w-full
              px-3 py-2 text-sm font-mono
              outline-none
              transition
              placeholder:font-sans
            "
          />

            </>
          ) : (
            <p
              data-testid="settings-no-key-needed"
              className="mb-3 rounded-md border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-xs text-[var(--text-dim)]"
            >
              Ollama runs on your machine and needs no API key. Start it with{" "}
              <code className="font-mono text-[var(--text)]">ollama serve</code>{" "}
              and pull a model &mdash;{" "}
              <code className="font-mono text-[var(--text)]">
                ollama pull llama3.2
              </code>
              . Selecting it above already makes it active — no key and no
              Save needed. Note that tool calling is per-model, and a model
              without it will fail on the first edit.
            </p>
          )}

          <div className="mb-3 flex items-center justify-between text-xs">
            <a
              href={activeProviderEntry.keysUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="link-accent"
            >
              How to get a key →
            </a>
            <button
              type="button"
              onClick={handleTest}
              disabled={testDisabled}
              data-testid="settings-test-button"
              className="
                btn-tool rounded
                px-2.5 py-1
                font-mono text-[10px] uppercase tracking-wider
                transition
                disabled:cursor-not-allowed disabled:opacity-50
              "
            >
              {test.kind === "running" ? "Testing…" : "Test"}
            </button>
          </div>

          {test.kind === "ok" ? (
            <p
              data-testid="settings-test-ok"
              className="mb-3 rounded-md border border-[var(--success)]/35 bg-[var(--success)]/10 px-3 py-1.5 text-xs text-[var(--success)]"
            >
              Connected — this model can call tools.
            </p>
          ) : null}
          {test.kind === "no-tools" ? (
            <div
              data-testid="settings-test-no-tools"
              role="alert"
              className="mb-3 rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-1.5 text-xs text-[var(--warning)]"
            >
              <p>
                Connected, but <span className="font-mono">{test.model}</span>{" "}
                did not call the tool it was offered. Every edit in edytlab is a
                tool call, so editing will not work with this model — pick one
                that supports tool calling.
              </p>
              {test.detail ? (
                <p className="mt-1 font-mono text-[10px] opacity-70">
                  {test.detail}
                </p>
              ) : null}
            </div>
          ) : null}
          {test.kind === "model-missing" ? (
            <div
              data-testid="settings-test-model-missing"
              role="alert"
              className="mb-3 rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-1.5 text-xs text-[var(--warning)]"
            >
              <p>
                <span className="font-mono">{test.model}</span> isn't offered by{" "}
                {activeProviderEntry.label} — it may have been retired. Pick
                another model below.
              </p>
              {test.model !== DEFAULT_MODEL_BY_PROVIDER[provider] ? (
                <button
                  type="button"
                  data-testid="settings-test-use-default"
                  onClick={() => handleModelChange(DEFAULT_MODEL_BY_PROVIDER[provider])}
                  className="mt-1 rounded border border-[var(--warning)]/40 px-2 py-0.5 font-mono text-[10px] hover:bg-[var(--warning)]/20"
                >
                  Use {DEFAULT_MODEL_BY_PROVIDER[provider]}
                </button>
              ) : null}
            </div>
          ) : null}
          {test.kind === "err" ? (
            <p
              data-testid="settings-test-error"
              role="alert"
              className="mb-3 rounded-md border border-[var(--danger)]/40 bg-[var(--danger)]/10 px-3 py-1.5 text-xs text-[var(--danger)]"
            >
              {test.message}
            </p>
          ) : null}

          <label
            htmlFor="settings-base-url-input"
            className="mb-1.5 block font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]"
          >
            Base URL
          </label>
          <input
            id="settings-base-url-input"
            value={baseUrl}
            onChange={(e) => {
              setBaseUrl(e.target.value);
              // Typed over: the user means this, whatever was stored.
              setBaseUrlKnown(true);
            }}
            data-testid="settings-base-url-input"
            aria-label="Base URL"
            spellCheck={false}
            autoComplete="off"
            placeholder={defaultBaseUrl}
            className="
              field
              mb-1 w-full
              px-3 py-2 text-sm font-mono
              outline-none
              transition
            "
          />
          {baseUrlUnread !== null && !baseUrlKnown ? (
            <p
              data-testid="settings-base-url-unread"
              role="status"
              className="mb-1 text-xs text-[var(--warning)]"
            >
              Couldn&apos;t read the saved base URL from the system keychain, so
              Save leaves it as it is. ({baseUrlUnread})
            </p>
          ) : null}
          <p className="mb-4 text-xs text-[var(--text-faint)]">
            Point this provider anywhere that speaks its API — a local
            server on another port, a gateway, a proxy. Leave empty for
            the default. LM Studio and most local runners serve an
            OpenAI-compatible API, usually at{" "}
            <code className="font-mono">http://localhost:1234/v1</code>.
          </p>

          <label
            htmlFor="settings-model-input"
            className="mb-1.5 block font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]"
          >
            Model
          </label>
          <input
            id="settings-model-input"
            list="settings-model-suggestions"
            value={model}
            onChange={(e) => handleModelChange(e.target.value)}
            onFocus={() => setModelFocused(true)}
            onBlur={() => setModelFocused(false)}
            data-testid="settings-model-input"
            aria-label="Model"
            placeholder={DEFAULT_MODEL_BY_PROVIDER[provider]}
            className="
              field
              mb-1 w-full
              px-3 py-2 text-sm font-mono
              outline-none
              transition
            "
          />
          <datalist id="settings-model-suggestions" data-testid="settings-model-datalist">
            {suggestions.map((s) => (
              <option key={s.id} value={s.id} label={s.label} />
            ))}
          </datalist>
          {modelUnread !== null && !modelKnown ? (
            <p
              data-testid="settings-model-unread"
              role="status"
              className="mb-1 text-xs text-[var(--warning)]"
            >
              Couldn&apos;t read the saved model from the system keychain, so
              Save leaves it as it is. ({modelUnread})
            </p>
          ) : null}
          {modelHint ? (
            <p
              data-testid={
                modelHintKind === "error"
                  ? "settings-model-error"
                  : modelHintKind === "fallback"
                  ? "settings-model-fallback"
                  : "settings-model-hint"
              }
              className={
                "mb-4 text-xs " +
                (modelHintKind === "error" || modelHintKind === "fallback"
                  ? "text-[var(--warning)]"
                  : "text-[var(--text-faint)]")
              }
            >
              {modelHint}
            </p>
          ) : (
            <div className="mb-4" />
          )}
          {modelUnlisted ? (
            <div
              data-testid="settings-model-unlisted"
              role="status"
              className="-mt-2 mb-4 rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-1.5 text-xs text-[var(--warning)]"
            >
              <p>
                <span className="font-mono">{savedModel}</span> is not in
                Anthropic's current model list — it may have been retired. The
                default is{" "}
                <span className="font-mono">
                  {DEFAULT_MODEL_BY_PROVIDER.anthropic}
                </span>
                .
              </p>
              <button
                type="button"
                data-testid="settings-model-use-default"
                onClick={() => handleModelChange(DEFAULT_MODEL_BY_PROVIDER.anthropic)}
                className="mt-1 rounded border border-[var(--warning)]/40 px-2 py-0.5 font-mono text-[10px] hover:bg-[var(--warning)]/20"
              >
                Use default
              </button>
            </div>
          ) : null}

          {supportsEffort ? (
            <>
              <label
                htmlFor="settings-effort-select"
                className="mb-1.5 block font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]"
              >
                Reasoning effort
              </label>
              <select
                id="settings-effort-select"
                value={effort}
                onChange={(e) => {
                  setEffort(asEffort(e.target.value));
                  setEffortKnown(true);
                }}
                data-testid="settings-effort-select"
                aria-label="Reasoning effort"
                className="
                  field
                  mb-1 w-full
                  px-3 py-2 text-sm
                  outline-none
                  transition
                "
              >
                {EFFORT_OPTIONS.map((o) => (
                  <option key={o.value || "default"} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>
              {effortUnread !== null && !effortKnown ? (
                <p
                  data-testid="settings-effort-unread"
                  role="status"
                  className="mb-1 text-xs text-[var(--warning)]"
                >
                  Couldn&apos;t read the saved level from the system keychain,
                  so Save leaves it as it is. ({effortUnread})
                </p>
              ) : null}
              <p
                data-testid="settings-effort-hint"
                className="mb-4 text-xs text-[var(--text-faint)]"
              >
                Higher effort is slower and uses more tokens.
              </p>
            </>
          ) : null}

          {/* Not in the first-run card: it is already as tall as a small
              window allows (settings-fits-window.spec.ts), and the line
              is no use to someone who has not set a key yet. The speech
              model's row follows it (#383): the one place a model is
              downloaded, and only when Download is pressed. */}
          {mode === "panel" ? (
            <>
              <MlRuntimeLine />
              <SpeechModelRow />
            </>
          ) : null}

          </>
          ) : null}
          </div>
        </div>

        {tab === "account" ? (
          // Outside the scrolling body, so Save is reachable without
          // scrolling and a failed save's message is read where it was
          // clicked, not somewhere below the fold.
          <div
            data-testid="settings-footer"
            className="shrink-0 border-t border-[var(--border)] px-5 pb-4 pt-3"
          >
            {saveError ? (
              <p
                data-testid="settings-save-error"
                role="alert"
                className="mb-3 rounded-md border border-[var(--danger)]/40 bg-[var(--danger)]/10 px-3 py-1.5 text-xs text-[var(--danger)]"
              >
                {saveError}
              </p>
            ) : null}
            {savedSessionOnly !== null ? (
              <p
                data-testid="settings-save-session-only"
                role="status"
                className="mb-3 rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-1.5 text-xs text-[var(--warning)]"
              >
                {savedSessionOnly}
              </p>
            ) : null}

            <div className="flex items-center justify-between gap-2">
              {mode === "panel" ? (
                <button
                  type="button"
                  onClick={handleClear}
                  data-testid="settings-clear-button"
                  className="btn-danger rounded-md px-3 py-1.5 text-sm transition"
                >
                  Clear key
                </button>
              ) : (
                <span />
              )}
              {savedSessionOnly !== null ? (
                <button
                  type="button"
                  onClick={handleContinue}
                  data-testid="settings-continue-button"
                  className="btn-primary px-4 py-1.5 text-sm transition"
                >
                  Continue
                </button>
              ) : (
                <button
                  type="button"
                  onClick={handleSave}
                  disabled={saveDisabled}
                  data-testid="settings-save-button"
                  className="btn-primary px-4 py-1.5 text-sm transition"
                >
                  {saving ? "Saving…" : "Save & Continue"}
                </button>
              )}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

interface SettingsTabButtonProps {
  id: SettingsTab;
  label: string;
  active: boolean;
  onClick: () => void;
  /** Bring this tab clear of the strip's edge fade (#392). */
  onReveal: (tabEl: HTMLElement) => void;
}

function SettingsTabButton({
  id,
  label,
  active,
  onClick,
  onReveal,
}: SettingsTabButtonProps) {
  // Whether a pointer press is in progress on this tab. A press reveals
  // on click rather than on focus; see the note on the tab strip (#392).
  const pressed = useRef(false);
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      data-testid={`settings-tab-${id}`}
      onPointerDown={() => {
        pressed.current = true;
      }}
      onFocus={(e) => {
        if (!pressed.current) onReveal(e.currentTarget);
      }}
      onBlur={() => {
        pressed.current = false;
      }}
      onClick={(e) => {
        pressed.current = false;
        onReveal(e.currentTarget);
        onClick();
      }}
      className={
        // The selected bar is `.tab-indicator`'s pseudo-element, not a
        // border, so it adds no height: `pb-2` is the old `py-1.5` plus
        // the 2px the border took, and the strip is the size it was. No
        // weight change either, which would move every tab beside it.
        "tab-indicator shrink-0 whitespace-nowrap rounded-t-md px-2 pb-2 pt-1.5 text-sm transition " +
        (active
          ? "text-[var(--text)]"
          : "text-[var(--text-dim)] hover:text-[var(--text)]")
      }
    >
      {label}
    </button>
  );
}
