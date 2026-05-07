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

import { useCallback, useState } from "react";

import {
  clearApiKey,
  setApiKey,
  testApiKey,
} from "../lib/tauri-bridge";

/** localStorage key under which the chosen model is persisted. */
export const MODEL_STORAGE_KEY = "edytlab.model";

/** URL surfaced by the "How to get a key" link. */
export const ANTHROPIC_KEYS_URL =
  "https://console.anthropic.com/settings/keys";

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
  const [model, setModel] = useState<string>(() => {
    if (typeof window === "undefined") return DEFAULT_MODEL;
    return window.localStorage.getItem(MODEL_STORAGE_KEY) ?? DEFAULT_MODEL;
  });
  const [test, setTest] = useState<TestState>({ kind: "idle" });
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

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
      await setApiKey(key);
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
  }, [key, saving, onSaved]);

  const handleTest = useCallback(async () => {
    if (!key.trim()) return;
    setTest({ kind: "running" });
    try {
      await testApiKey(key);
      setTest({ kind: "ok" });
    } catch (err) {
      setTest({ kind: "err", message: String(err) });
    }
  }, [key]);

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
            edytlab needs an Anthropic API key to power the assistant.
            Your key is stored in your OS keychain — never on disk in
            plaintext.
          </p>
        ) : null}

        <label className="mb-1 block text-xs uppercase tracking-wide text-zinc-400">
          Anthropic API key
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
          placeholder="sk-ant-…"
          data-testid="settings-key-input"
          aria-label="Anthropic API key"
          className="mb-2 w-full rounded border border-zinc-700 bg-zinc-950 px-2 py-1.5 text-sm focus:border-zinc-500 focus:outline-none"
        />

        <div className="mb-3 flex items-center justify-between text-xs">
          <a
            href={ANTHROPIC_KEYS_URL}
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
