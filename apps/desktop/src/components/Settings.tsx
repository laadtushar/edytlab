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
 * user knows their id), and the suggestion list shows curated entries
 * pulled from the live catalogue. Per-provider selections are
 * persisted in localStorage under `edytlab.model.<provider>` so
 * switching provider preserves each side's choice.
 */

import { useCallback, useEffect, useState } from "react";

import {
  clearApiKey,
  listModelsFor,
  setApiKeyFor,
  setActiveModel,
  setActiveProvider,
  testApiKeyFor,
  type ModelInfo,
  type ProviderId,
} from "../lib/tauri-bridge";
import { AgentProfilesEditor } from "./AgentProfilesEditor";
import { MemoryEditor } from "./MemoryEditor";
import { SkillsEditor } from "./SkillsEditor";

/** Settings panel tabs. The blocking onboarding flow is locked to
 *  "account"; the panel mode lets the user switch. Phases 1-4 ship
 *  "account", "memory", "skills", "agents"; phase 5 adds "mcp". */
type SettingsTab = "account" | "memory" | "skills" | "agents";

/** Legacy (single-provider) localStorage key. Migrated to the
 * per-provider slot below on first read. */
export const LEGACY_MODEL_STORAGE_KEY = "edytlab.model";

/** Per-provider localStorage prefix for the chosen model id. */
export const MODEL_STORAGE_KEY_PREFIX = "edytlab.model.";

/** localStorage key under which the chosen provider is mirrored. */
export const PROVIDER_STORAGE_KEY = "edytlab.provider";

/** URL surfaced by the "How to get a key" link, per provider. */
export const ANTHROPIC_KEYS_URL =
  "https://console.anthropic.com/settings/keys";
export const OPENROUTER_KEYS_URL = "https://openrouter.ai/keys";
export const OPENAI_KEYS_URL = "https://platform.openai.com/api-keys";

/** Per-provider default model id used when the user has no saved
 * selection. Mirrored loosely from the Rust `provider.rs` defaults. */
const DEFAULT_MODEL_BY_PROVIDER: Record<ProviderId, string> = {
  anthropic: "claude-sonnet-4-6",
  openrouter: "anthropic/claude-sonnet-4-6",
  openai: "gpt-4o-mini",
};

/** Provider catalogue surfaced in the Settings picker. */
const PROVIDERS: ReadonlyArray<{
  id: ProviderId;
  label: string;
  keyPlaceholder: string;
  keysUrl: string;
}> = [
  {
    id: "anthropic",
    label: "Anthropic",
    keyPlaceholder: "sk-ant-...",
    keysUrl: ANTHROPIC_KEYS_URL,
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
];

const DEFAULT_PROVIDER: ProviderId = "anthropic";

/** Build the localStorage key for a provider's chosen model. */
function modelStorageKey(provider: ProviderId): string {
  return `${MODEL_STORAGE_KEY_PREFIX}${provider}`;
}

/**
 * Read the model id stored for `provider`, applying a one-shot
 * migration from the legacy single-provider `edytlab.model` slot the
 * first time the per-provider slot is empty for the Anthropic provider.
 */
function readStoredModel(provider: ProviderId): string {
  if (typeof window === "undefined") return DEFAULT_MODEL_BY_PROVIDER[provider];
  const ns = window.localStorage.getItem(modelStorageKey(provider));
  if (ns && ns.trim()) return ns;
  if (provider === "anthropic") {
    // Best-effort migration: if the user previously saved a model
    // under the legacy key, treat it as the Anthropic choice.
    const legacy = window.localStorage.getItem(LEGACY_MODEL_STORAGE_KEY);
    if (legacy && legacy.trim()) {
      window.localStorage.setItem(modelStorageKey("anthropic"), legacy);
      window.localStorage.removeItem(LEGACY_MODEL_STORAGE_KEY);
      return legacy;
    }
  }
  return DEFAULT_MODEL_BY_PROVIDER[provider];
}

/** Result of the most recent Test-button click. */
type TestState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "ok" }
  | { kind: "err"; message: string };

export interface SettingsProps {
  /**
   * Determines layout and whether the user is allowed to dismiss the
   * panel. `blocking` shows a full-screen overlay with no Close button.
   */
  mode: "blocking" | "panel";
  /** Called when the user successfully saves a (validated) API key. */
  onSaved: () => void;
  /** Called when the user dismisses a `mode="panel"` Settings panel. */
  onClose?: () => void;
  /** Called after the user clears the stored key. */
  onCleared?: () => void;
}

export function Settings({
  mode,
  onSaved,
  onClose,
  onCleared,
}: SettingsProps) {
  const [key, setKey] = useState("");
  const [provider, setProvider] = useState<ProviderId>(() => {
    if (typeof window === "undefined") return DEFAULT_PROVIDER;
    const stored = window.localStorage.getItem(PROVIDER_STORAGE_KEY);
    if (
      stored === "openrouter" ||
      stored === "anthropic" ||
      stored === "openai"
    ) {
      return stored;
    }
    return DEFAULT_PROVIDER;
  });
  const [model, setModel] = useState<string>(() => readStoredModel(provider));
  const [test, setTest] = useState<TestState>({ kind: "idle" });
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [tab, setTab] = useState<SettingsTab>("account");

  // Persist the provider choice so the picker remembers which radio
  // was selected when the modal re-opens.
  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(PROVIDER_STORAGE_KEY, provider);
    }
  }, [provider]);

  // Refresh the model field whenever the provider changes — pull from
  // localStorage so each provider's last selection sticks across
  // toggles.
  useEffect(() => {
    setModel(readStoredModel(provider));
  }, [provider]);

  /**
   * Fetch the model catalogue for the active provider. Re-runs on
   * provider change and when the user finishes typing a key (OpenAI
   * needs the key to authenticate the catalogue endpoint).
   */
  const fetchCatalogue = useCallback(
    async (p: ProviderId, k: string | undefined) => {
      try {
        const list = await listModelsFor(p, k);
        setModels(list);
        setModelsError(null);
      } catch (err) {
        setModels([]);
        setModelsError(String(err));
      }
    },
    [],
  );

  useEffect(() => {
    void fetchCatalogue(provider, key.trim() || undefined);
  }, [provider, fetchCatalogue]); // intentionally NOT key — see below

  // Re-fetch when the user *stops* typing a key (debounce 400ms). This
  // gives OpenAI a chance to populate suggestions once the user has
  // pasted their key, without spamming the endpoint on every keystroke.
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
      setProvider(next);
      // Wipe the input key + test state — they belonged to the
      // previous provider.
      setKey("");
      setTest({ kind: "idle" });
      try {
        await setActiveProvider(next);
      } catch (err) {
        setSaveError(String(err));
      }
    },
    [provider],
  );

  const handleModelChange = useCallback(
    (next: string) => {
      setModel(next);
      if (typeof window !== "undefined") {
        window.localStorage.setItem(modelStorageKey(provider), next);
      }
      // Push the choice down to the Rust state so the next agent
      // rebuild reads it. Errors are non-fatal; the agent rebuild
      // happens on Save anyway.
      void setActiveModel(provider, next).catch(() => {
        /* swallow — local state is the source of truth here */
      });
    },
    [provider],
  );

  const handleSave = useCallback(async () => {
    if (!key.trim() || saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      await setApiKeyFor(provider, key);
      // Push the model selection through too so the rebuilt agent
      // honours it on the very next turn.
      if (model.trim()) {
        try {
          await setActiveModel(provider, model);
        } catch {
          /* non-fatal */
        }
      }
      setKey("");
      setTest({ kind: "idle" });
      onSaved();
    } catch (err) {
      setSaveError(String(err));
    } finally {
      setSaving(false);
    }
  }, [key, provider, model, saving, onSaved]);

  const handleTest = useCallback(async () => {
    if (!key.trim()) return;
    setTest({ kind: "running" });
    try {
      await testApiKeyFor(provider, key);
      setTest({ kind: "ok" });
    } catch (err) {
      setTest({ kind: "err", message: String(err) });
    }
  }, [key, provider]);

  const handleClear = useCallback(async () => {
    try {
      await clearApiKey();
      setKey("");
      setTest({ kind: "idle" });
      onCleared?.();
    } catch (err) {
      setSaveError(String(err));
    }
  }, [onCleared]);

  const activeProviderEntry =
    PROVIDERS.find((p) => p.id === provider) ?? PROVIDERS[0];

  const saveDisabled = !key.trim() || saving;
  const testDisabled = !key.trim() || test.kind === "running";

  const containerClass =
    mode === "blocking"
      ? "fixed inset-0 z-50 flex items-center justify-center bg-[radial-gradient(ellipse_at_center,rgba(0,0,0,0.85),rgba(0,0,0,0.97))] backdrop-blur-sm app-fade-in"
      : "fixed inset-0 z-40 flex items-center justify-center bg-black/55 backdrop-blur-sm app-fade-in";

  // Format suggestion labels. OpenRouter entries are
  // `provider/model — display_name`; native providers just show
  // `id — display_name`. We dedupe by id since Rust returns sorted.
  const suggestions = models.map((m) => {
    const label =
      m.display_name && m.display_name !== m.id
        ? `${m.id} — ${m.display_name}`
        : m.id;
    return { id: m.id, label };
  });

  // Hint string: when the user has loaded a catalogue we show the
  // count, otherwise the inline error. The hint sits beneath the
  // combo so users always know whether suggestions are coming from
  // the live catalogue.
  const modelHint =
    modelsError !== null
      ? `Couldn't fetch model list (${modelsError}); type the model id manually.`
      : models.length > 0
      ? `${models.length} model${models.length === 1 ? "" : "s"} available — start typing for suggestions.`
      : "";

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      data-testid="settings"
      data-mode={mode}
      className={containerClass}
    >
      <div
        className="
          relative w-[30rem] max-w-[90vw] overflow-hidden
          rounded-xl border border-[var(--border-strong)]
          bg-[var(--surface-elev)]
          shadow-[0_30px_80px_-20px_rgba(0,0,0,0.7),0_0_0_1px_rgba(255,255,255,0.02)_inset]
        "
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] px-5 py-4">
          <div className="flex items-baseline gap-2">
            <h2 className="font-[var(--font-serif)] text-2xl leading-none text-[var(--text)]">
              {mode === "blocking" ? (
                <>
                  <span className="italic text-[var(--accent)]">Welcome</span>{" "}
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
              onClick={onClose}
              data-testid="settings-close"
              aria-label="Close settings"
              className="
                rounded-md p-1.5
                text-[var(--text-dim)]
                hover:bg-[var(--surface-elev-2)] hover:text-[var(--text)]
                transition
              "
            >
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
                <path d="M3 3l8 8M11 3l-8 8" />
              </svg>
            </button>
          ) : null}
        </div>

        {/* Tab nav — only in panel mode. The blocking onboarding flow
            stays locked to the Account tab so a first-time user can't
            wander into Memory before configuring a key. */}
        {mode === "panel" ? (
          <div
            data-testid="settings-tabs"
            role="tablist"
            className="flex items-center gap-1 border-b border-[var(--border)] px-5 pt-3"
          >
            <SettingsTabButton
              id="account"
              label="Account"
              active={tab === "account"}
              onClick={() => setTab("account")}
            />
            <SettingsTabButton
              id="memory"
              label="Memory"
              active={tab === "memory"}
              onClick={() => setTab("memory")}
            />
            <SettingsTabButton
              id="skills"
              label="Skills"
              active={tab === "skills"}
              onClick={() => setTab("skills")}
            />
            <SettingsTabButton
              id="agents"
              label="Agents"
              active={tab === "agents"}
              onClick={() => setTab("agents")}
            />
          </div>
        ) : null}

        <div className="px-5 py-4">
          {tab === "memory" && mode === "panel" ? <MemoryEditor /> : null}
          {tab === "skills" && mode === "panel" ? <SkillsEditor /> : null}
          {tab === "agents" && mode === "panel" ? (
            <AgentProfilesEditor />
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
                      ? "border-[var(--accent)]/55 bg-[var(--accent-soft)] text-[var(--accent)]"
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
              mb-2 w-full
              rounded-md border border-[var(--border-strong)]
              bg-[var(--surface)]
              px-3 py-2 text-sm font-mono text-[var(--text)]
              outline-none
              transition
              placeholder:text-[var(--text-faint)] placeholder:font-sans
              focus:border-[var(--accent)]/55
              focus:shadow-[0_0_0_3px_var(--accent-soft)]
            "
          />

          <div className="mb-3 flex items-center justify-between text-xs">
            <a
              href={activeProviderEntry.keysUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="text-[var(--accent)] hover:underline"
            >
              How to get a key →
            </a>
            <button
              type="button"
              onClick={handleTest}
              disabled={testDisabled}
              data-testid="settings-test-button"
              className="
                rounded border border-[var(--border-strong)]
                bg-[var(--surface)]
                px-2.5 py-1
                font-mono text-[10px] uppercase tracking-wider text-[var(--text-dim)]
                transition
                hover:border-[var(--accent)]/50 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
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
              Key looks good.
            </p>
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
            data-testid="settings-model-input"
            aria-label="Model"
            placeholder={DEFAULT_MODEL_BY_PROVIDER[provider]}
            className="
              mb-1 w-full
              rounded-md border border-[var(--border-strong)]
              bg-[var(--surface)]
              px-3 py-2 text-sm font-mono text-[var(--text)]
              outline-none
              transition
              placeholder:text-[var(--text-faint)]
              focus:border-[var(--accent)]/55
              focus:shadow-[0_0_0_3px_var(--accent-soft)]
            "
          />
          <datalist id="settings-model-suggestions" data-testid="settings-model-datalist">
            {suggestions.map((s) => (
              <option key={s.id} value={s.id} label={s.label} />
            ))}
          </datalist>
          {modelHint ? (
            <p
              data-testid={
                modelsError !== null
                  ? "settings-model-error"
                  : "settings-model-hint"
              }
              className={
                "mb-4 text-xs " +
                (modelsError !== null
                  ? "text-[var(--warning)]"
                  : "text-[var(--text-faint)]")
              }
            >
              {modelHint}
            </p>
          ) : (
            <div className="mb-4" />
          )}

          {saveError ? (
            <p
              data-testid="settings-save-error"
              role="alert"
              className="mb-3 rounded-md border border-[var(--danger)]/40 bg-[var(--danger)]/10 px-3 py-1.5 text-xs text-[var(--danger)]"
            >
              {saveError}
            </p>
          ) : null}

          <div className="flex items-center justify-between gap-2 pt-1">
            {mode === "panel" ? (
              <button
                type="button"
                onClick={handleClear}
                data-testid="settings-clear-button"
                className="
                  rounded-md border border-[var(--danger)]/40
                  px-3 py-1.5 text-sm text-[var(--danger)]
                  transition
                  hover:bg-[var(--danger)]/10
                "
              >
                Clear key
              </button>
            ) : (
              <span />
            )}
            <button
              type="button"
              onClick={handleSave}
              disabled={saveDisabled}
              data-testid="settings-save-button"
              className="
                rounded-md
                bg-[var(--accent)]
                px-4 py-1.5
                text-sm font-medium text-[var(--bg)]
                shadow-[0_4px_12px_-4px_var(--accent-glow)]
                transition
                hover:bg-[#ffa05f]
                disabled:cursor-not-allowed disabled:bg-[var(--surface-elev-2)] disabled:text-[var(--text-faint)] disabled:shadow-none
              "
            >
              {saving ? "Saving…" : "Save & Continue"}
            </button>
          </div>
          </>
          ) : null}
        </div>
      </div>
    </div>
  );
}

interface SettingsTabButtonProps {
  id: SettingsTab;
  label: string;
  active: boolean;
  onClick: () => void;
}

function SettingsTabButton({
  id,
  label,
  active,
  onClick,
}: SettingsTabButtonProps) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      data-testid={`settings-tab-${id}`}
      onClick={onClick}
      className={
        "rounded-t-md border-b-2 px-3 py-1.5 text-sm transition " +
        (active
          ? "border-[var(--accent)] text-[var(--text)]"
          : "border-transparent text-[var(--text-dim)] hover:text-[var(--text)]")
      }
    >
      {label}
    </button>
  );
}
