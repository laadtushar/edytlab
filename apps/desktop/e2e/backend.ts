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

import type { RenderWarning, TrackSummary } from "../src/lib/tauri-bridge";

import { fixturePath, fixtureSeconds } from "./audio-fixtures";

/**
 * One command's answer: its result, the string it fails with, or — for
 * testing an ordering — a result the test releases when it chooses (see
 * `App.release`).
 */
export type SimpleAnswer = { ok: unknown } | { reject: string } | { deferred: string };

/**
 * A simple answer, or, for a command whose answer depends on what it is
 * asked about, one simple answer for each value of one of its arguments
 * (see {@link byArg}). A value the test did not list is a call it did not
 * anticipate, and is reported as one.
 */
export type Answer = SimpleAnswer | { byArg: string; answers: Record<string, SimpleAnswer> };

export type Backend = Record<string, Answer>;

export const ok = (value: unknown): SimpleAnswer => ({ ok: value });
export const reject = (message: string): SimpleAnswer => ({ reject: message });
export const deferred = (name: string): SimpleAnswer => ({ deferred: name });
export const byArg = (arg: string, answers: Record<string, SimpleAnswer>): Answer => ({
  byArg: arg,
  answers,
});

/**
 * `CommandError::NoSession`'s `Display`, from `commands.rs`. Every
 * command's `CmdResult<T>` is `Result<T, String>`, so the frontend
 * receives exactly this string.
 */
export const NO_SESSION = "no session loaded; call open_project first";

/**
 * `get_transcript` with no transcript: `TranscriptOut::none()` in
 * `commands.rs`. No words, and a source that allows no cut (#384).
 */
export const NO_TRANSCRIPT = { words: [], source: { kind: "unknown" } };

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

/** Bytes of the speech model as pinned in `crates/ml-pipeline/models.json`. */
export const SPEECH_MODEL_BYTES = 138_644_904;

/**
 * `MlModelStatusDto` (`commands.rs`) for `whisper-base`: a flat object
 * tagged by `state`, with `bytes` on disk and the pinned `size`. Override
 * the fields a state carries (`received` and `total`, or `reason`).
 */
export function speechModelStatus(
  fields:
    | { state: "not_downloaded"; bytes?: number }
    | { state: "downloading"; received: number }
    | { state: "ready" }
    | { state: "failed"; reason: string; bytes?: number },
): Record<string, unknown> {
  const base = { model: "whisper-base", size: SPEECH_MODEL_BYTES };
  switch (fields.state) {
    case "not_downloaded":
      return { ...base, state: fields.state, bytes: fields.bytes ?? 0 };
    case "downloading":
      return {
        ...base,
        state: fields.state,
        received: fields.received,
        total: SPEECH_MODEL_BYTES,
        bytes: fields.received,
      };
    case "ready":
      return { ...base, state: fields.state, bytes: SPEECH_MODEL_BYTES };
    case "failed":
      return { ...base, state: fields.state, reason: fields.reason, bytes: fields.bytes ?? 0 };
  }
}

/**
 * The speech model buttons' commands, for the spec that presses them.
 * Each answers what the command answers when it is called on a model in
 * the state before it: `download_ml_model` on one that is not downloaded
 * starts it and answers `downloading` at 0 (`ModelDownloads::start_download`);
 * `cancel_ml_model_download` answers the state at the time of the call
 * (`ModelDownloads::cancel` sets the flag and returns the status, which for
 * a running download is its slot's current `received`: the end arrives as an
 * event), so a spec that cancels a download passes the bytes it had
 * received; and `remove_ml_model` on a model that is not running answers
 * `not_downloaded` with 0 bytes.
 */
export function speechModelCommands({ received = 0 }: { received?: number } = {}): Backend {
  return {
    download_ml_model: ok(speechModelStatus({ state: "downloading", received: 0 })),
    cancel_ml_model_download: ok(speechModelStatus({ state: "downloading", received })),
    remove_ml_model: ok(speechModelStatus({ state: "not_downloaded" })),
  };
}

/**
 * `ChatGptStatusDto` (`chatgpt.rs`) for a person who never signed in:
 * `status_dto` of a `SessionState::SignedOut` session that is not signing in.
 * `email` is left out of a signed-out status, there is no `reason` and no
 * token to be stored, and no message is kept.
 */
export const CHATGPT_SIGNED_OUT = {
  state: "signed_out",
  email: null,
  reason: null,
  tokenStorage: null,
  pendingMessage: false,
} as const;

/**
 * The same, signed in: `SessionState::SignedIn` with the token in the system
 * keychain (`token_storage_dto` of `TokenStorage::Stored` where
 * `ai::keychain::persistence()` is `Durable`).
 */
export const CHATGPT_SIGNED_IN = {
  state: "signed_in",
  email: "person@example.com",
  reason: null,
  tokenStorage: { kind: "keychain", persistent: true, reason: null },
  pendingMessage: false,
} as const;

/**
 * The ChatGPT sign-in commands (#474), for a spec that presses them. Each is
 * a `CmdResult` of the DTO in `chatgpt.rs`:
 *
 * - `chatgpt_status`: `ChatGptHandle::status`, the status now.
 * - `chatgpt_sign_in`: resolves when the sign-in is over, with the status at
 *   the end of it (a cancel resolves with the status as it then stands).
 * - `chatgpt_cancel_sign_in`: `Ok(())`, which serialises as `null`.
 * - `chatgpt_sign_out`: a `ChatGptEndDto` of a sign-out. No token was left to
 *   revoke, so `revocation` is `nothing_to_revoke`; the token is removed, and
 *   with nothing loose there is no `notice`. `registrationForgotten` is
 *   skipped for a sign-out.
 * - `chatgpt_forget_registration`: the same for a start over, which adds
 *   `registrationForgotten` and always has a `notice`: the old entry may
 *   remain in ChatGPT's settings.
 * - `chatgpt_take_pending_message`: `None` while no message is kept.
 *
 * Status changes arrive on the `chatgpt://status` event, which a spec fires
 * itself.
 */
export function chatgptCommands(
  status: Record<string, unknown> = CHATGPT_SIGNED_OUT,
): Backend {
  return {
    chatgpt_status: ok(status),
    chatgpt_sign_in: ok(CHATGPT_SIGNED_IN),
    chatgpt_cancel_sign_in: ok(null),
    chatgpt_sign_out: ok({
      status: CHATGPT_SIGNED_OUT,
      revocation: "nothing_to_revoke",
      tokenRemoved: true,
      notice: null,
    }),
    chatgpt_forget_registration: ok({
      status: CHATGPT_SIGNED_OUT,
      revocation: "nothing_to_revoke",
      tokenRemoved: true,
      registrationForgotten: true,
      notice:
        "edytlab has forgotten its registration, and the next sign-in registers it again. The old entry may still be listed in ChatGPT settings, under Security and login, Sign in with ChatGPT; you can disconnect it there.",
    }),
    chatgpt_take_pending_message: ok(null),
  };
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
    // `ml_runtime_status_dto(ml_pipeline::runtime::info())`, which the
    // panel's Account tab asks for (the first-run card does not). The
    // answer here is a build without the ONNX Runtime library (one from
    // source, or a release up to v0.6.1; the installers after it carry
    // it, #383) and no `ORT_DYLIB_PATH`, so `runtime::info` is
    // `Error::MissingRuntime { searched }`, which the DTO maps to
    // `state: "missing"` with the places `candidate_paths()` listed, as
    // strings: here a deb install's resource directory and then its
    // executable's.
    ml_runtime_status: ok({
      state: "missing",
      searched: ["/usr/lib/edytlab/libonnxruntime.so", "/usr/bin/libonnxruntime.so"],
    }),
    // `ml_model_status("whisper-base")` on a machine that never downloaded
    // it, which the panel's Account tab asks for (the first-run card does
    // not). `ModelDownloads::status`: no transfer is running, so the disk
    // decides; `ModelStore::on_disk` finds no file under
    // `<app data>/models/whisper-base`, so `complete` is false and `bytes`
    // 0, and the state is `NotDownloaded`. `size` is the sum of the three
    // pinned files in `crates/ml-pipeline/models.json`: 82,451,730 +
    // 53,712,708 + 2,480,466. Serialised through `MlModelStatusDto`
    // (`commands.rs`): `state` is snake_case, the rest camelCase. The
    // buttons' commands (`download_ml_model`, `cancel_ml_model_download`,
    // `remove_ml_model`) are answered by `speechModelCommands` for the spec
    // that presses them; nothing here calls them.
    ml_model_status: ok(speechModelStatus({ state: "not_downloaded" })),
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
          reasoning_levels: [],
        },
        {
          id: "claude-haiku-5-5",
          display_name: "Claude Haiku 5.5",
          context_length: 1_000_000,
          provider_hint: null,
          reasoning_levels: [],
        },
        {
          id: "claude-opus-5-5",
          display_name: "Claude Opus 5.5",
          context_length: 1_000_000,
          provider_hint: null,
          reasoning_levels: [],
        },
        {
          id: "claude-fable-5-1",
          display_name: "Claude Fable 5.1",
          context_length: 1_000_000,
          provider_hint: null,
          reasoning_levels: [],
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
    // `AppState::plan_first` starts from `~/.edytlab/settings.json`, and a
    // fresh install has none: off (#569).
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
 * `get_transcript` answers no words (and a source that allows no cut),
 * because "not transcribed yet" is an ordinary state and not a fault.
 */
function emptyProject(): Backend {
  return {
    list_tracks: reject(NO_SESSION),
    list_markers: reject(NO_SESSION),
    get_transcript: ok(NO_TRANSCRIPT),
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
    get_transcript: ok(NO_TRANSCRIPT),
    // `save_view_state` returns `CmdResult<()>`, which serialises as
    // `null`. The app calls it, debounced, once there is a head.
    save_view_state: ok(null),
    // `render_warnings_of(&node.state)`: the effects in the node's stored
    // chains that a render leaves out. A project this build saved holds
    // only effects the renderer runs (`add_effect` refuses the rest), so
    // none. A test of a project an older build saved overrides it with
    // `leftOut(..)`. The app asks for the node it shows, whenever it
    // changes.
    get_render_warnings: ok([]),
  };
}

/**
 * One entry of `get_render_warnings`' answer: a switched-on effect in a
 * stored chain that the render leaves out (`RenderWarning`,
 * `commands.rs`).
 *
 * `reason` is `not_yet_streamable` for the kinds `NOT_YET_STREAMING` lists
 * in `effect_chain.rs` (reverb, echo, eq, ...) and `unknown` for a kind
 * nobody knows. `effect_index` counts the stored chain, bypassed entries
 * included.
 */
export function leftOut(
  scope: RenderWarning["scope"],
  kind: string,
  { effectIndex = 0, reason = "not_yet_streamable" as RenderWarning["reason"] } = {},
): RenderWarning {
  return { scope, effect_index: effectIndex, kind, reason };
}

/**
 * An A/B compare of two nodes, as the backend answers it.
 *
 * `prepare_compare` renders each side into the project's preview cache
 * (`<project>/.audiograph/previews/<node>.wav`) and returns those paths,
 * what each side's render leaves out (`render_warnings_of`), and
 * `compare_id`: `CompareHolds::begin`'s count, which starts at 1 with each
 * launch. The window's own paths here are fixtures, since this backend
 * renders nothing. `end_compare` is `CmdResult<()>`, so `null`, whether or
 * not the id named the open compare. The A/B bar calls it when it closes.
 */
export function comparing(sides: {
  a_path: string;
  b_path: string;
  a_warnings: RenderWarning[];
  b_warnings: RenderWarning[];
}): Backend {
  return {
    prepare_compare: ok({ ...sides, compare_id: 1 }),
    end_compare: ok(null),
  };
}

/**
 * `accept_b`, the A/B bar's "Accept B ✓", for the one node `b` a spec
 * accepts. The command makes the node's audio present (`materialize`),
 * sets the session head to it (`Store::set_head`) and answers with it
 * (`b_id.to_hex()`, a `CmdResult<String>`). Another node is a call the
 * spec did not anticipate. The session the next `list_tracks` reads is
 * the spec's to say: `App.become`.
 */
export function acceptingB(b: string): Backend {
  return { accept_b: byArg("b", { [b]: ok(b) }) };
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

/**
 * `set_head_to` on a store that holds the nodes `held` and no longer has
 * the nodes `gone` (a compaction removed them, #595).
 *
 * `set_head_to_inner` first puts back any audio a sweep removed
 * (`materialize`), which for a node it cannot read is nothing:
 * `rederive::missing_paths` lists no paths for it. Then `Store::set_head`
 * refuses an id that has no node file with `Error::NodeNotFound(hex)`,
 * which `CommandError::Session` prints as `session error: node not found:
 * <hex>`. A node the store holds is answered with its own id
 * (`id.to_hex()`).
 */
export function headMovesWithin({ held, gone }: { held: string[]; gone: string[] }): Backend {
  return {
    set_head_to: byArg("nodeId", {
      ...Object.fromEntries(held.map((id) => [id, ok(id)])),
      ...Object.fromEntries(
        gone.map((id) => [id, reject(`session error: node not found: ${id}`)]),
      ),
    }),
  };
}

/** A node id as the backend formats one: 32 bytes, as 64 hex digits. */
export function nodeId(n: number): string {
  return n.toString(16).padStart(64, "0");
}
