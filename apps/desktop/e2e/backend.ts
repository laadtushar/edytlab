/**
 * What the Rust backend answers, for the states a test starts from.
 *
 * Each answer is taken from the command's own source rather than from
 * what looks plausible. A default that the real backend never returns
 * makes a test pass against an app that does not exist; each comment
 * says where the answer comes from. Two did not, in the first draft of
 * this file, and a review caught both: `get_transcript` was made to
 * fail with `NoSession`, which it never does, and a track carried an
 * `audio_path` with no clips, a shape `list_tracks` never produces.
 *
 * Where the real answer comes from files the release bundles —
 * templates, skills — it is computed from those same files, so adding a
 * template changes the answer here too.
 *
 * Shapes follow `src/lib/tauri-bridge.ts`, which is the frontend's own
 * statement of what each command returns.
 */

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import type { TrackSummary } from "../src/lib/tauri-bridge";

/** One command's answer: its result, or the string it fails with. */
export type Answer = { ok: unknown } | { reject: string };

export type Backend = Record<string, Answer>;

export const ok = (value: unknown): Answer => ({ ok: value });
export const reject = (message: string): Answer => ({ reject: message });

/**
 * `CommandError::NoSession`'s `Display`, from `commands.rs`. Every
 * command's `CmdResult<T>` is `Result<T, String>`, so the frontend
 * receives exactly this string.
 */
export const NO_SESSION = "no session loaded; call open_project first";

/** `src-tauri/resources`, which `tauri.conf.json` bundles. */
const RESOURCES = join(
  dirname(fileURLToPath(import.meta.url)),
  "../src-tauri/resources",
);

/**
 * `list_templates`: every `resources/templates/*.json`, reduced to its
 * `name` and `description`. The real order is `read_dir`'s, which no
 * platform promises, so this sorts by file name to stay deterministic.
 */
function bundledTemplates(): { name: string; description: string }[] {
  const dir = join(RESOURCES, "templates");
  return readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .sort()
    .map((f) => {
      const t = JSON.parse(readFileSync(join(dir, f), "utf8"));
      return { name: t.name ?? "", description: t.description ?? "" };
    });
}

/**
 * `install_bundled_skills` on a first launch: it copies every bundled
 * `.md` into `~/.edytlab/skills` and returns how many.
 */
function bundledSkillCount(): number {
  return readdirSync(join(RESOURCES, "skills")).filter((f) => f.endsWith(".md")).length;
}

/**
 * What Settings asks for when it opens, for a user who has never
 * changed a provider setting.
 */
function providerCommands(): Backend {
  return {
    // `AppState::new` starts on `ai::ANTHROPIC_ID`.
    get_active_provider: ok("anthropic"),
    // `model_for(..).unwrap_or_default()`: nothing chosen yet.
    get_active_model: ok(""),
    // No override stored in the keychain.
    get_base_url_for: ok(null),
    // `ANTHROPIC_DEFAULT_BASE_URL` in `crates/ai/src/provider.rs`.
    default_base_url_for: ok("https://api.anthropic.com"),
    // `anthropic_models()` in `crates/ai/src/models.rs` — a static
    // catalogue, so it answers without a key or a network. Serialised
    // through `ModelInfoDto`.
    list_models_for: ok([
      {
        id: "claude-sonnet-4-6",
        display_name: "Claude Sonnet 4.6 (default)",
        context_length: 200_000,
        provider_hint: null,
      },
      {
        id: "claude-haiku-4-5-20251001",
        display_name: "Claude Haiku 4.5 (cheap mode)",
        context_length: 200_000,
        provider_hint: null,
      },
      {
        id: "claude-opus-4-1-20250805",
        display_name: "Claude Opus 4.1",
        context_length: 200_000,
        provider_hint: null,
      },
    ]),
  };
}

/**
 * The commands the app calls on every boot, found by booting it with
 * no answers at all and recording what it asked for.
 */
function bootCommands({ hasKey }: { hasKey: boolean }): Backend {
  return {
    ...providerCommands(),
    // `AppState::plan_first` starts as `AtomicBool::new(false)`.
    get_plan_first: ok(false),
    // `get_sync_lock` returns `Ok(false)` rather than failing when no
    // session is open, so the window can still draw.
    get_sync_lock: ok(false),
    // Without a key, the app opens Settings as onboarding — which is
    // what calls `providerCommands`.
    has_api_key: ok(hasKey),
    install_bundled_skills: ok(bundledSkillCount()),
    list_recent_projects: ok([]),
    list_templates: ok(bundledTemplates()),
  };
}

/**
 * A project with no history yet.
 *
 * `lib.rs` opens a default project store at every launch, so a project
 * is always open — but `Store::open` on a fresh directory has no
 * `HEAD`. `list_tracks` and `list_markers` turn that into `NoSession`;
 * `get_transcript` answers an empty list, because "not transcribed yet"
 * is an ordinary state and not a fault.
 */
function emptyProject(): Backend {
  return {
    list_tracks: reject(NO_SESSION),
    list_markers: reject(NO_SESSION),
    get_transcript: ok([]),
  };
}

/** The very first launch: no key, and nothing in the project yet. */
export function firstRun(): Backend {
  return { ...bootCommands({ hasKey: false }), ...emptyProject() };
}

/** A user who has set a key but not yet loaded anything. */
export function readyToLoad(): Backend {
  return { ...bootCommands({ hasKey: true }), ...emptyProject() };
}

/**
 * The session-reading commands once the project has a history, holding
 * these tracks.
 *
 * All of them move together. A backend that lists tracks while still
 * refusing markers with `NoSession` is not one the app can ever talk
 * to, and a test written against it proves nothing.
 */
export function sessionWith(tracks: TrackSummary[]): Backend {
  return {
    list_tracks: ok(tracks),
    list_markers: ok([]),
    get_transcript: ok([]),
    // `save_view_state` returns `CmdResult<()>`, which serialises as
    // `null`. The app calls it, debounced, once there is a head.
    save_view_state: ok(null),
  };
}

/**
 * A track holding one file, as `list_tracks` reports it.
 *
 * One clip, because that is what `audio_path` requires: `list_tracks`
 * sets it to the clip's `source_path` for exactly one clip, to a
 * flattened WAV for several, and to `None` for none. The id is a UUID
 * because `TrackId` is one.
 */
export function trackFor(audioPath: string, seconds: number): TrackSummary {
  return {
    id: "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f",
    name: "Track 1",
    muted: false,
    gain_db: 0,
    pan: 0,
    soloed: false,
    audio_path: audioPath,
    clips: [
      { start_sec: 0, length_sec: seconds, source_path: audioPath, volume_envelope: [] },
    ],
  };
}

/**
 * What the app calls once the user has selected something.
 *
 * `set_selection_context` stores the range for the agent's next turn and
 * returns `Ok(())`, session or not: it touches only `AppState`. The app
 * pushes it 250 ms after every selection change.
 */
export function selecting(): Backend {
  return { set_selection_context: ok(null) };
}

/** A node id as the backend formats one: 32 bytes, as 64 hex digits. */
export function nodeId(n: number): string {
  return n.toString(16).padStart(64, "0");
}
