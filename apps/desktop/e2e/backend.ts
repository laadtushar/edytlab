/**
 * What the Rust backend answers, for the states a test starts from.
 *
 * Each answer is taken from the command's own source rather than from
 * what looks plausible. A default that the real backend never returns
 * makes a test pass against an app that does not exist; where the
 * answer depends on a real default, the comment says where it lives.
 *
 * Shapes follow `src/lib/tauri-bridge.ts`, which is the frontend's own
 * statement of what each command returns.
 */

import type { TrackSummary } from "../src/lib/tauri-bridge";

/** One command's answer: its result, or the string it fails with. */
export type Answer = { ok: unknown } | { reject: string };

export type Backend = Record<string, Answer>;

/**
 * `CommandError::NoSession`'s `Display`, from `commands.rs`. Commands
 * that read the session return it when no project is open, and every
 * command's `CmdResult<T>` is `Result<T, String>`, so the frontend
 * receives exactly this string.
 */
export const NO_SESSION = "no session loaded; call open_project first";

export const ok = (value: unknown): Answer => ({ ok: value });
export const reject = (message: string): Answer => ({ reject: message });

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
    // The count of skills newly installed. Nothing the app renders
    // reads it.
    install_bundled_skills: ok(0),
    list_recent_projects: ok([]),
    list_templates: ok([]),
  };
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
 * A project with no history yet.
 *
 * `lib.rs` opens a default project store at every launch, so a project
 * is always open — but `Store::open` on a fresh directory has no
 * `HEAD`, and the session-reading commands turn `store.head() == None`
 * into `NoSession`. So they fail on a first launch even though nothing
 * is wrong, exactly as here.
 */
function emptyProject(): Backend {
  return {
    list_tracks: reject(NO_SESSION),
    list_markers: reject(NO_SESSION),
    get_transcript: reject(NO_SESSION),
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

/** A track as `list_tracks` reports it: one source file, unity gain. */
export function trackFor(audioPath: string, name = "Track 1"): TrackSummary {
  return {
    id: `track-${name.replace(/\s+/g, "-").toLowerCase()}`,
    name,
    muted: false,
    gain_db: 0,
    pan: 0,
    soloed: false,
    audio_path: audioPath,
    clips: [],
  };
}

/**
 * The session-reading commands once the project has a history — after
 * the first load, or when `Store::open` found a `HEAD` on disk.
 *
 * All of them move together. A backend that lists tracks while still
 * refusing the transcript with `NoSession` is not one the app can ever
 * talk to, and a test written against it proves nothing.
 */
export function session(tracks: TrackSummary[]): Backend {
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
 * A project that already holds these tracks when the app starts.
 *
 * This is a returning user: `Store::open` restores `HEAD` from disk, so
 * whatever they had loaded last time is there at boot. They have set a
 * key, so onboarding stays out of the way.
 */
export function projectWith(tracks: TrackSummary[]): Backend {
  return { ...bootCommands({ hasKey: true }), ...session(tracks) };
}
