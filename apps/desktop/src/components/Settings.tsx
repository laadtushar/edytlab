/**
 * Settings — API key + model selection UI (Phase 1, M13).
 *
 * Two rendering modes:
 *  - `mode="blocking"`: full-screen modal that covers the chat until the
 *    user provides a key. Has no Close button (the only escape is to
 *    save a working key).
 *  - `mode="panel"`: opened from the gear icon in the header. Adds a
 *    Close button and a "Clear key" affordance.
 *
 * Security posture:
 *  - The API key lives in component state only while the user is typing.
 *    Once `setApiKey` resolves we wipe the input field. The "Test"
 *    button hands the key to a Rust command, so the renderer never
 *    talks to Anthropic directly.
 *  - Model choice is persisted in `localStorage` for now; the agent
 *    rebuild path that would honour it is deferred to a follow-up.
 *    Surfacing the dropdown today still satisfies M13's spec of
 *    "change model" without committing to the backend wiring.
 */

import { useCallback, useEffect, useState } from "react";

import {
  clearApiKey,
  setApiKeyFor,
  setActiveProvider,
  testApiKeyFor,
  type ProviderId,
} from "../lib/tauri-bridge";

/** localStorage key under which the chosen model is persisted. */
export const MODEL_STORAGE_KEY = "edytlab.model";

/** localStorage key under which the chosen provider is mirrored. */
export const PROVIDER_STORAGE_KEY = "edytlab.provider";

/** URL surfaced by the "How to get a key" link, per provider. */
export const ANTHROPIC_KEYS_URL =
  "https://console.anthropic.com/settings/keys";

export const OPENROUTER_KEYS_URL = "https://openrouter.ai/keys";

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
];

const DEFAULT_PROVIDER: ProviderId = "anthropic";

/** Models exposed in the dropdown. Keep in sync with `crates/ai/src/prompt.rs`. */
const MODELS = [
  { id: "claude-sonnet-4-6", label: "Sonnet 4.6 (default)" },
  { id: "claude-haiku-4-5", label: "Haiku 4.5 (cheap mode)" },
] as const;

const DEFAULT_MODEL = MODELS[0].id;

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
    return stored === "openrouter" ? "openrouter" : DEFAULT_PROVIDER;
  });
  const [model, setModel] = useState<string>(() => {
    if (typeof window === "undefined") return DEFAULT_MODEL;
    return window.localStorage.getItem(MODEL_STORAGE_KEY) ?? DEFAULT_MODEL;
  });
  const [test, setTest] = useState<TestState>({ kind: "idle" });
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Persist the provider choice to localStorage so the picker remembers
  // which radio was selected when the modal re-opens, even before the
  // user has saved a key. The Rust backend remains the source of truth
  // for the *active* provider once a key is saved.
  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(PROVIDER_STORAGE_KEY, provider);
    }
  }, [provider]);

  const handleProviderChange = useCallback(
    async (next: ProviderId) => {
      if (next === provider) return;
      setProvider(next);
      // Wipe the input key and any in-flight test result — they belonged
      // to the previous provider, and persisting an Anthropic key under
      // OpenRouter (or vice versa) would be a credential-leak footgun.
      setKey("");
      setTest({ kind: "idle" });
      // Best-effort: tell the backend to switch active provider so the
      // agent (if any) is rebuilt against the new provider's stored
      // key. Failures are non-fatal — the user can still type a key
      // and Save, which switches as a side-effect.
      try {
        await setActiveProvider(next);
      } catch (err) {
        // Surface as save error so the user sees something went wrong;
        // the picker stays on the new value because the user explicitly
        // chose it.
        setSaveError(String(err));
      }
    },
    [provider],
  );

  const handleModelChange = useCallback((next: string) => {
    setModel(next);
    if (typeof window !== "undefined") {
      window.localStorage.setItem(MODEL_STORAGE_KEY, next);
    }
  }, []);

  const handleSave = useCallback(async () => {
    if (!key.trim() || saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      // Save against the picked provider explicitly. This also marks
      // it as active server-side, so the next chat turn routes through
      // its endpoint.
      await setApiKeyFor(provider, key);
      // Wipe the input the moment the key has been persisted — we don't
      // want it lingering in component state any longer than needed.
      setKey("");
      setTest({ kind: "idle" });
      onSaved();
    } catch (err) {
      setSaveError(String(err));
    } finally {
      setSaving(false);
    }
  }, [key, provider, saving, onSaved]);

  const handleTest = useCallback(async () => {
    if (!key.trim()) return;
    setTest({ kind: "running" });
    try {
      // Validate against the picked provider's endpoint, not whichever
      // one happens to be active server-side.
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
      ? "fixed inset-0 z-50 flex items-center justify-center bg-black/80"
      : "fixed inset-0 z-40 flex items-center justify-center bg-black/40";

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      data-testid="settings"
      data-mode={mode}
      className={containerClass}
    >
      <div className="w-[28rem] max-w-[90vw] rounded-lg border border-zinc-700 bg-zinc-900 p-5 text-zinc-100 shadow-xl">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-base font-semibold">
            {mode === "blocking" ? "Welcome to edytlab" : "Settings"}
          </h2>
          {mode === "panel" && onClose ? (
            <button
              type="button"
              onClick={onClose}
              data-testid="settings-close"
              className="rounded px-2 py-1 text-sm text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
              aria-label="Close settings"
            >
              Close
            </button>
          ) : null}
        </div>

        {mode === "blocking" ? (
          <p className="mb-3 text-sm text-zinc-400">
            edytlab needs an LLM API key to power the assistant. Pick a
            provider below; your key is stored in your OS keychain —
            never on disk in plaintext.
          </p>
        ) : null}

        <fieldset
          className="mb-3"
          data-testid="settings-provider-picker"
          aria-label="LLM provider"
        >
          <legend className="mb-1 block text-xs uppercase tracking-wide text-zinc-400">
            Provider
          </legend>
          <div className="flex gap-3">
            {PROVIDERS.map((p) => (
              <label
                key={p.id}
                className={`flex flex-1 cursor-pointer items-center gap-2 rounded border px-2 py-1.5 text-sm ${
                  provider === p.id
                    ? "border-blue-500 bg-blue-900/20 text-blue-100"
                    : "border-zinc-700 text-zinc-200 hover:border-zinc-500"
                }`}
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
                  className="accent-blue-500"
                />
                {p.label}
              </label>
            ))}
          </div>
        </fieldset>

        <label className="mb-1 block text-xs uppercase tracking-wide text-zinc-400">
          {activeProviderEntry.label} API key
        </label>
        <input
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={key}
          onChange={(e) => {
            setKey(e.target.value);
            // Reset Test state when the key changes — the result no
            // longer applies.
            if (test.kind !== "idle") setTest({ kind: "idle" });
          }}
          placeholder={activeProviderEntry.keyPlaceholder}
          data-testid="settings-key-input"
          aria-label={`${activeProviderEntry.label} API key`}
          className="mb-2 w-full rounded border border-zinc-700 bg-zinc-950 px-2 py-1.5 text-sm focus:border-zinc-500 focus:outline-none"
        />

        <div className="mb-3 flex items-center justify-between text-xs">
          <a
            href={activeProviderEntry.keysUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="text-blue-400 hover:underline"
          >
            How to get a key
          </a>
          <button
            type="button"
            onClick={handleTest}
            disabled={testDisabled}
            data-testid="settings-test-button"
            className="rounded border border-zinc-700 px-2 py-1 text-zinc-200 hover:bg-zinc-800 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {test.kind === "running" ? "Testing…" : "Test"}
          </button>
        </div>

        {test.kind === "ok" ? (
          <p
            data-testid="settings-test-ok"
            className="mb-3 rounded border border-green-700 bg-green-900/40 px-2 py-1 text-xs text-green-200"
          >
            Key looks good.
          </p>
        ) : null}
        {test.kind === "err" ? (
          <p
            data-testid="settings-test-error"
            role="alert"
            className="mb-3 rounded border border-red-700 bg-red-900/40 px-2 py-1 text-xs text-red-200"
          >
            {test.message}
          </p>
        ) : null}

        <label className="mb-1 block text-xs uppercase tracking-wide text-zinc-400">
          Model
        </label>
        <select
          value={model}
          onChange={(e) => handleModelChange(e.target.value)}
          data-testid="settings-model-select"
          aria-label="Model"
          className="mb-4 w-full rounded border border-zinc-700 bg-zinc-950 px-2 py-1.5 text-sm focus:border-zinc-500 focus:outline-none"
        >
          {MODELS.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label}
            </option>
          ))}
        </select>

        {saveError ? (
          <p
            data-testid="settings-save-error"
            role="alert"
            className="mb-3 rounded border border-red-700 bg-red-900/40 px-2 py-1 text-xs text-red-200"
          >
            {saveError}
          </p>
        ) : null}

        <div className="flex items-center justify-between gap-2">
          {mode === "panel" ? (
            <button
              type="button"
              onClick={handleClear}
              data-testid="settings-clear-button"
              className="rounded border border-red-800 px-3 py-1.5 text-sm text-red-300 hover:bg-red-900/30"
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
            className="rounded bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
