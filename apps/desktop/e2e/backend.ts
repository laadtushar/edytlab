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

import { fixturePath, fixtureSeconds } from "./audio-fixtures";

/**
 * One command's answer: its result, the string it fails with, or — for
 * testing an ordering — a result the test releases when it chooses (see
 * `App.release`).
 */
export type Answer = { ok: unknown } | { reject: string } | { deferred: string };

export type Backend = Record<string, Answer>;

export const ok = (value: unknown): Answer => ({ ok: value });
export const reject = (message: string): Answer => ({ reject: message });
export const deferred = (name: string): Answer => ({ deferred: name });

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
    // `active_provider_answer(state.keychain_read_error(), ..)`: with a
    // clean startup, `AppState::new` starts on `ai::ANTHROPIC_ID`. After a
    // startup the keychain refused it rejects with the reason instead.
    get_active_provider: ok("anthropic"),
    // `active_model_answer(models_unread_reason(..), model_for(..))`: with
    // a clean startup and nothing chosen yet, the empty string. After a
    // startup the keychain refused it rejects with the reason instead.
    get_active_model: ok(""),
    // `stored_setting_unless_refused(.., try_load_base_url(..))`: no
    // override stored, so `None`. A keychain that cannot be read rejects
    // with the reason instead, and so does one that refused at launch,
    // without being asked.
    get_base_url_for: ok(null),
    // `ANTHROPIC_DEFAULT_BASE_URL` in `crates/ai/src/provider.rs`.
    default_base_url_for: ok("https://api.anthropic.com"),
    // `stored_setting_unless_refused(.., try_load_effort(..))` mapped
    // through `e.as_str()`: no slot stored, so `None` (the model's own
    // default). An unreadable keychain rejects with the reason, and so
    // does one that refused at launch, without being asked.
    get_effort_for: ok(null),
    // `list_models_at`'s Anthropic arm with no key saved, which is what a
    // fresh profile has: `anthropic_fallback_models()` in
    // `crates/ai/src/models.rs`, uncached, with the reason as the notice.
    // Serialised through `ModelCatalogueDto` (`commands.rs`). After a
    // startup the keychain refused, `catalogue_inputs` rejects with the
    // refusal's note before any of that.
    list_models_for: ok({
      models: [
        {
          id: "claude-sonnet-5-5",
          display_name: "Claude Sonnet 5.5 (default)",
          context_length: 1_000_000,
          provider_hint: null,
        },
        {
          id: "claude-haiku-5-5",
          display_name: "Claude Haiku 5.5",
          context_length: 1_000_000,
          provider_hint: null,
        },
        {
          id: "claude-opus-5-5",
          display_name: "Claude Opus 5.5",
          context_length: 1_000_000,
          provider_hint: null,
        },
        {
          id: "claude-fable-5-1",
          display_name: "Claude Fable 5.1",
          context_length: 1_000_000,
          provider_hint: null,
        },
      ],
      source: "fallback",
      notice: "no API key saved",
    }),
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
    // what calls `providerCommands`. In the app it first waits for
    // startup's keychain reads (`AppState::startup_reads_settled`), which
    // run off the setup thread; a test of that wait answers `deferred`.
    has_api_key: ok(hasKey),
    // `ai::keychain::persistence()`: `Durable` on macOS and Windows, and
    // on Linux whenever a Secret Service answers. A test of the
    // no-Secret-Service warning overrides this.
    get_keychain_persistence: ok({ persistent: true, reason: null }),
    install_bundled_skills: ok(bundledSkillCount()),
    // Read at every launch, to put the user back where they were.
    // `get_view_state` needs a project directory, not a head, and the
    // default project always has one; with no `view.json` it reads
    // `ViewState::default()`, which serialises as `{}`. A test that needs
    // a saved view overrides this.
    get_view_state: ok({}),
    // Read at every launch and every project open, beside the head
    // (#453). `get_head_trail` is `session::trail::read_trail` on the open
    // project's directory, which never fails: with no `trail.json` it is
    // `HeadTrail::default()`, serialised as a null head and two empty
    // lists. A test of a project that was closed with a path overrides it
    // with what the earlier run saved.
    get_head_trail: ok({ head: null, trail: [], redo: [] }),
    // `save_head_trail` is `CmdResult<()>`, so `null`, whether the trail
    // was written or dropped for not belonging to the open project. The
    // app calls it after every head move, with no wait; like
    // `get_view_state` it needs a project directory, not a head.
    save_head_trail: ok(null),
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
    // `get_session_head`: `store.head().ok_or(CommandError::NoSession)`.
    get_session_head: reject(NO_SESSION),
  };
}

/**
 * A Linux machine with no Secret Service running: the keychain answers
 * every read, but nothing saved to it survives a reboot.
 *
 * `get_keychain_persistence` is `ai::keychain::persistence().into()`, and
 * `probe` turns an unreachable service (the keyring's `PlatformFailure`)
 * into `SessionOnly { reason: e.to_string() }`, which `From` maps to
 * `persistent: false` with that reason. The reason is the text a real run
 * produced with `DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent`, the
 * way a box with no session bus answers (release-candidate run of
 * e2b4c30). It is not decoration: the warning Settings draws is that
 * sentence plus this reason, and its length is what decides how tall the
 * dialog is.
 */
export function sessionOnlyKeychain(): Backend {
  return {
    get_keychain_persistence: ok({
      persistent: false,
      reason:
        "Platform secure storage failure: DBus error: Failed to connect to socket /nonexistent: No such file or directory",
    }),
  };
}

/**
 * Where a save landed, as the save commands answer: a
 * `KeychainPersistenceDto` (`commands.rs`), which `landed(..)` builds from
 * the `Persistence` that `ai::keychain::save_*` return. `Durable` is
 * `{ persistent: true, reason: null }`, a save that fell back to the
 * kernel keyring `{ persistent: false, reason }`, with the reason of the
 * first write that did. A clear writes nothing and is always durable.
 */
export const DURABLE_SAVE = { persistent: true, reason: null } as const;

/**
 * What Settings' Save sends, answered as the backend does when the
 * keychain keeps what it is given: each of these is a
 * `CmdResult<KeychainPersistenceDto>` (see {@link DURABLE_SAVE}). Save
 * writes the base URL and the reasoning level once their reads have
 * answered (the defaults here do), then the key, then the model.
 */
export function savingSettings(): Backend {
  return {
    set_base_url_for: ok(DURABLE_SAVE),
    set_effort_for: ok(DURABLE_SAVE),
    set_api_key_for: ok(DURABLE_SAVE),
    set_active_model: ok(DURABLE_SAVE),
  };
}

/**
 * The same saves on a keychain that lost its Secret Service after launch:
 * every write falls back to the kernel keyring. The reason is the text
 * `write_to` gives (`e.to_string()` of the keyring's `PlatformFailure`), the
 * same a real run of {@link sessionOnlyKeychain}'s probe produced.
 */
export function savingSettingsSessionOnly(): Backend {
  const fellBack = {
    persistent: false,
    reason:
      "Platform secure storage failure: DBus error: Failed to connect to socket /nonexistent: No such file or directory",
  };
  return {
    set_base_url_for: ok(fellBack),
    set_effort_for: ok(fellBack),
    set_api_key_for: ok(fellBack),
    set_active_model: ok(fellBack),
  };
}

/**
 * What the Settings panel's other tabs ask for when opened on a fresh
 * profile.
 *
 * - `get_project_meta`: `read_meta` of the default project, which has no
 *   `project.json`, so `ProjectMeta::from_dir` — the folder's name
 *   (`default-project`, `lib.rs`) and nothing else, since every other
 *   field is skipped when empty.
 * - `read_memory`: `MemoryStore::read`, `""` for a file never written.
 * - `list_skills`, `list_agent_profiles`, `list_mcp_servers`: no entries.
 *   A first launch installs the bundled skills, but the editor is a
 *   fixed `h-[28rem]` whatever its list holds, so the dialog is no
 *   different for it.
 * - `get_active_agent_profile`: `AppState::active_agent_profile`, `None`
 *   until one is chosen.
 */
export function settingsTabs(): Backend {
  return {
    get_project_meta: ok({ name: "default-project" }),
    read_memory: ok(""),
    list_skills: ok([]),
    list_agent_profiles: ok([]),
    get_active_agent_profile: ok(null),
    list_mcp_servers: ok([]),
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
 * A project that already holds these tracks when the app starts: a
 * returning user.
 *
 * `lib.rs` reopens the default project at every launch and
 * `Store::open` restores `HEAD` from disk, so whatever they loaded last
 * time is there at boot. `get_session_head` answers with that head —
 * `commands.rs` returns `head.to_hex()` whenever the store has one.
 * They have set a key, so onboarding stays out of the way.
 */
export function projectWith(tracks: TrackSummary[], head: string): Backend {
  return {
    ...bootCommands({ hasKey: true }),
    ...sessionWith(tracks),
    get_session_head: ok(head),
  };
}

/**
 * A track holding one file, as `list_tracks` reports it.
 *
 * One clip, the whole file, at zero — a file just loaded — because that
 * is what this `audio_path` requires: `list_tracks` hands over the
 * clip's `source_path` only for such a clip, a flattened WAV for
 * anything else (see `placedTrack`), and `None` for no clips. The id is
 * a UUID because `TrackId` is one.
 */
export function trackFor(
  audioPath: string,
  seconds: number,
  { name = "Track 1", id = "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f" } = {},
): TrackSummary {
  return {
    id,
    name,
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
 * A track holding one clip placed `startSec` into the session.
 *
 * `list_tracks` hands a single clip's source over only when the clip is
 * its whole source at zero (`tools::lane_audio_path`). A placed clip is
 * not, so `audio_path` is the flattened lane file instead: silence to
 * `startSec`, then the clip. `lanePath` is a fixture holding exactly
 * that, since this backend cannot flatten anything itself.
 */
export function placedTrack({
  source,
  startSec,
  lengthSec,
  lanePath,
  name,
  id,
}: {
  source: string;
  startSec: number;
  lengthSec: number;
  lanePath: string;
  name: string;
  id: string;
}): TrackSummary {
  return {
    id,
    name,
    muted: false,
    gain_db: 0,
    pan: 0,
    soloed: false,
    audio_path: lanePath,
    clips: [
      { start_sec: startSec, length_sec: lengthSec, source_path: source, volume_envelope: [] },
    ],
  };
}

/**
 * A track with nothing on it yet: a template's track, or one that just
 * had its only clip removed. No clips, so `list_tracks` gives it no
 * `audio_path` at all.
 */
export function emptyTrack(name: string, id: string): TrackSummary {
  return {
    id,
    name,
    muted: false,
    gain_db: 0,
    pan: 0,
    soloed: false,
    audio_path: null,
    clips: [],
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

/** Track 1, holding the three-second tone. */
export function toneTrack(): TrackSummary {
  return trackFor(fixturePath("tone3s"), fixtureSeconds("tone3s"));
}

/**
 * `get_node`'s answer: a whole serialised `SessionNode`.
 *
 * Read from `crates/session/tests/snapshots/sample_node.json`, which the
 * Rust test `snapshot_roundtrips_byte_equal` holds byte-equal to what
 * `serde_json` writes for a `SessionNode` — so the shape here is the
 * backend's, not a guess at it. Only `id` and `parent` are set, to place
 * the node in the history a test builds; the app reads nothing else.
 */
export function sessionNode(id: string, parent: string | null): unknown {
  const snapshot = JSON.parse(
    readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), "../../../crates/session/tests/snapshots/sample_node.json"),
      "utf8",
    ),
  ) as Record<string, unknown>;
  return { ...snapshot, id, parent };
}

/** A node id as the backend formats one: 32 bytes, as 64 hex digits. */
export function nodeId(n: number): string {
  return n.toString(16).padStart(64, "0");
}
