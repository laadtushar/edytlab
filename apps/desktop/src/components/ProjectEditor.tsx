/**
 * ProjectEditor — what the project is called, and what it puts on the
 * files it exports (#225 §5).
 *
 * `get_project_meta` and `set_project_meta` shipped with no UI
 * reference at all, so the only way to name a project was to ask the
 * agent in a sentence — which needs a working LLM (#321) to do
 * something entirely deterministic.
 *
 * The tag fields are the other half. `render_final` writes
 * title/artist/album/year into the exported file and took every one as
 * a per-export argument, so the same answers had to be given again on
 * every render. These are the ones that belong to the project: an
 * episode's artist does not change between exports. Setting them here
 * makes every export carry them, and an explicit argument still wins
 * for a one-off.
 *
 * There is no `title` field on purpose. The project's **name** is the
 * title, and a second box holding the same thing is a pair that can
 * disagree — so the name's own label says what it does.
 */

import { useCallback, useEffect, useState } from "react";

import { getProjectMeta, setProjectMeta } from "../lib/tauri-bridge";
import type { ProjectMeta } from "../lib/tauri-bridge";

type Load =
  | { kind: "loading" }
  | { kind: "ready"; meta: ProjectMeta }
  | { kind: "none" }
  | { kind: "error"; message: string };

export function ProjectEditor() {
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [name, setName] = useState("");
  const [notes, setNotes] = useState("");
  const [artist, setArtist] = useState("");
  const [album, setAlbum] = useState("");
  const [year, setYear] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getProjectMeta()
      .then((meta) => {
        if (cancelled) return;
        setLoad({ kind: "ready", meta });
        setName(meta.name ?? "");
        setNotes(meta.notes ?? "");
        setArtist(meta.artist ?? "");
        setAlbum(meta.album ?? "");
        setYear(meta.year ?? "");
      })
      .catch((e) => {
        if (cancelled) return;
        // The command needs an open project. That is an ordinary
        // state, not a fault — say which it is rather than showing an
        // error for having opened Settings first.
        const msg = String(e);
        setLoad(
          /no project|not open/i.test(msg)
            ? { kind: "none" }
            : { kind: "error", message: msg },
        );
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Mirrors `set_project_meta`, which refuses an empty name — catching
  // it here turns a round-trip and an error into a disabled button.
  const nameOk = name.trim().length > 0;

  const save = useCallback(async () => {
    if (!nameOk || saving) return;
    setSaving(true);
    setSaveError(null);
    setSaved(false);
    try {
      const meta = await setProjectMeta(name, notes, artist, album, year);
      setLoad({ kind: "ready", meta });
      setSaved(true);
    } catch (e) {
      setSaveError(String(e));
    } finally {
      setSaving(false);
    }
  }, [nameOk, saving, name, notes, artist, album, year]);

  if (load.kind === "loading") {
    return (
      <p data-testid="project-editor-loading" className="text-sm text-[var(--text-dim)]">
        Loading…
      </p>
    );
  }

  if (load.kind === "none") {
    return (
      <p data-testid="project-editor-empty" className="text-sm text-[var(--text-dim)]">
        No project is open. Open or create one and its name and export
        tags can be set here.
      </p>
    );
  }

  if (load.kind === "error") {
    return (
      <p data-testid="project-editor-error" className="text-sm text-[var(--danger)]">
        {load.message}
      </p>
    );
  }

  const field = (
    label: string,
    hint: string,
    value: string,
    onChange: (v: string) => void,
    testid: string,
    extra?: { placeholder?: string; maxLength?: number },
  ) => (
    <label className="flex flex-col gap-1">
      <span className="font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
        {label}
      </span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        data-testid={testid}
        placeholder={extra?.placeholder}
        maxLength={extra?.maxLength}
        className="
          rounded-md border border-[var(--border-strong)]
          bg-[var(--surface)] px-2 py-1.5
          text-sm text-[var(--text)]
        "
      />
      <span className="text-[11px] leading-snug text-[var(--text-faint)]">
        {hint}
      </span>
    </label>
  );

  return (
    <div data-testid="project-editor" className="flex flex-col gap-4">
      <div className="flex flex-col gap-3">
        {field(
          "Name",
          "Shown in Recent projects, and used as the title tag on exports.",
          name,
          setName,
          "project-name",
        )}

        <label className="flex flex-col gap-1">
          <span className="font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            Notes
          </span>
          <textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            data-testid="project-notes"
            rows={3}
            className="
              rounded-md border border-[var(--border-strong)]
              bg-[var(--surface)] px-2 py-1.5
              text-sm text-[var(--text)]
            "
          />
          <span className="text-[11px] leading-snug text-[var(--text-faint)]">
            Yours, not the listener&apos;s — notes stay in the project and
            are never written to an exported file.
          </span>
        </label>
      </div>

      <div className="flex flex-col gap-3 border-t border-[var(--border)] pt-4">
        <p className="font-mono text-[9px] uppercase tracking-[0.2em] text-[var(--text-faint)]">
          Export tags
        </p>
        <p className="-mt-1 text-[11px] leading-snug text-[var(--text-faint)]">
          Written into every file this project exports, so they only have
          to be answered once. A single export can still override them.
          FLAC and MP3 carry tags; WAV has no standard place to put them.
        </p>

        {field(
          "Artist",
          "The performer or show.",
          artist,
          setArtist,
          "project-artist",
        )}
        {field("Album", "The series or collection.", album, setAlbum, "project-album")}
        {field("Year", "Four digits.", year, setYear, "project-year", {
          placeholder: "2026",
          maxLength: 4,
        })}
      </div>

      {saveError ? (
        <p data-testid="project-save-error" className="text-sm text-[var(--danger)]">
          {saveError}
        </p>
      ) : null}

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={save}
          disabled={!nameOk || saving}
          data-testid="project-save"
          className="
            rounded-md bg-[var(--accent)] px-3 py-1.5
            text-sm font-medium text-[var(--onyx-0,#07080b)]
            transition hover:bg-[#ffa05f]
            disabled:cursor-not-allowed disabled:opacity-40
          "
        >
          {saving ? "Saving…" : "Save"}
        </button>
        {!nameOk ? (
          <span data-testid="project-name-required" className="text-[11px] text-[var(--text-faint)]">
            A project needs a name.
          </span>
        ) : saved ? (
          <span data-testid="project-saved" className="text-[11px] text-[var(--text-faint)]">
            Saved.
          </span>
        ) : null}
      </div>
    </div>
  );
}
