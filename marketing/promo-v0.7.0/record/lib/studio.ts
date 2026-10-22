/**
 * A session the shots can edit, and the scripted assistant that edits it.
 *
 * Two things here are scripted, and everything above them is the app:
 *
 * 1. `Studio` stands in for the Rust store. It keeps the nodes, the head
 *    and the tracks, and answers the commands the window asks
 *    (`list_tracks`, `get_graph`, `render_preview`, ...) in the shapes the
 *    real commands return (`e2e/backend.ts` says where each comes from).
 * 2. `agentTurn` stands in for the model. With no API key it cannot be the
 *    real agent, so it raises the events the real agent loop raises
 *    (`agent://text-delta`, `tool-call`, `node-created`, `tool-call-end`,
 *    `done`) in the order `commands.rs` emits them, with the tool names and
 *    arguments of `docs/tools-reference.md`. A call's badge is `running`
 *    until its effect has been applied to the `Studio`, then it resolves.
 *
 * The user's side is not scripted at all: the shots type into the real
 * composer and press the real keys.
 */

import { createHash } from "node:crypto";

import type { GraphNode, TrackSummary } from "../../../../apps/desktop/src/lib/tauri-bridge";
import { byArg, deferred, ok, sessionWith, type Backend } from "../../../../apps/desktop/e2e/backend";
import { expect, type App } from "../../../../apps/desktop/e2e/fixtures";

import type { Stage } from "./stage";

/** A node id as the store makes one: 64 hex digits of a hash, not a counter. */
export const hid = (seed: string): string => createHash("sha256").update(`edytlab-promo:${seed}`).digest("hex");

/** A track id, which is a UUID. */
export const uuid = (n: number): string => {
  const h = createHash("sha256").update(`edytlab-promo-track:${n}`).digest("hex");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-4${h.slice(13, 16)}-8${h.slice(17, 20)}-${h.slice(20, 32)}`;
};

/** A whole-file clip at zero, which is what a loaded file is. */
export function loadedTrack(
  n: number,
  name: string,
  path: string,
  seconds: number,
  extra: Partial<TrackSummary> = {},
): TrackSummary {
  return {
    id: uuid(n),
    name,
    muted: false,
    gain_db: 0,
    pan: 0,
    soloed: false,
    audio_path: path,
    clips: [{ start_sec: 0, length_sec: seconds, source_path: path, volume_envelope: [] }],
    ...extra,
  };
}

export class Studio {
  nodes: GraphNode[] = [];
  head = "";
  tracks: TrackSummary[] = [];
  /** Where `render_preview` finds the mix of a node. */
  mixes = new Map<string, string>();
  private readonly t0 = Date.now();

  /** Add a node on the head (or on `parent`) and make it the head. */
  commit(tool: string, opts: { label?: string | null; parent?: string | null; agoSec?: number; seed?: string } = {}): string {
    const parent = opts.parent === undefined ? (this.head || null) : opts.parent;
    const id = hid(opts.seed ?? `${this.nodes.length}:${tool}:${parent ?? "root"}`);
    this.nodes.push({
      id,
      parent,
      label: opts.label ?? null,
      tool,
      created_at: new Date(this.t0 - (opts.agoSec ?? 0) * 1000).toISOString(),
    });
    this.head = id;
    return id;
  }

  /** `name_node`: a label, and no new node. */
  name(id: string, label: string): void {
    const n = this.nodes.find((x) => x.id === id);
    if (!n) throw new Error(`no node ${id}`);
    n.label = label;
  }

  /** What the Rust commands answer for the session as it is now. */
  answers(): Backend {
    return {
      ...sessionWith(this.tracks),
      // `get_session_head`: `head.to_hex()`.
      get_session_head: ok(this.head),
      // `get_graph`: every node, as `GraphSummary`.
      get_graph: ok({ nodes: this.nodes.map((n) => ({ ...n })), head: this.head }),
      // `render_preview`: the path of the node's mix in the preview cache.
      render_preview: byArg(
        "node",
        Object.fromEntries([...this.mixes].map(([node, path]) => [node, ok(path)])),
      ),
    };
  }

  /** Tell the app's mock backend what it now holds. */
  async sync(app: App): Promise<void> {
    await app.become(this.answers());
  }
}

// -- the scripted assistant -------------------------------------------------

export interface ToolStep {
  tool: string;
  /**
   * What the call does to the session. Runs after the badge has shown
   * `running` for `runMs`. If it moves the head, `node-created` follows, as
   * the real loop sends it, before the badge resolves.
   */
  effect?: () => void;
  runMs?: number;
}

export type Beat = ToolStep | { say: string } | { pause: number };

let toolSeq = 0;
const toolId = () => `toolu_01${String(++toolSeq).padStart(6, "0")}promo`;

/**
 * Stream `text` as the model does: a few words at a time, a beat apart. The
 * app joins the deltas into one bubble and commits it at `done`.
 */
export async function stream(stage: Stage, text: string, msPerChunk = 66): Promise<void> {
  const chunks = text.match(/\S+\s*/g) ?? [];
  let i = 0;
  while (i < chunks.length) {
    const n = 1 + (i % 3 === 0 ? 1 : 0);
    const piece = chunks.slice(i, i + n).join("");
    i += n;
    await stage.app.emit("agent://text-delta", { text: piece });
    await stage.wait(msPerChunk * (0.7 + (0.6 * ((i * 7919) % 10)) / 10));
  }
}

/**
 * One turn of the assistant, after the user has sent a message: wait as a
 * model does before its first token, then play `beats`, then end the turn.
 * `send_message` must have been given `deferred(name)` before the user sent,
 * so the composer stays busy for the whole turn as it does in the app.
 */
export async function agentTurn(
  stage: Stage,
  studio: Studio,
  name: string,
  beats: Beat[],
  opts: { thinkMs?: number } = {},
): Promise<void> {
  const app = stage.app;
  await stage.wait(opts.thinkMs ?? 1300);
  for (const beat of beats) {
    if ("say" in beat) {
      await stream(stage, beat.say);
    } else if ("pause" in beat) {
      await stage.wait(beat.pause);
    } else {
      const id = toolId();
      await app.emit("agent://tool-call", { name: beat.tool, id });
      await stage.wait(beat.runMs ?? 520);
      const before = studio.head;
      beat.effect?.();
      await studio.sync(app);
      if (studio.head !== before) await app.emit("agent://node-created", { node_id: studio.head });
      await app.emit("agent://tool-call-end", { id, ok: true, not_run: false });
      await stage.wait(180);
    }
  }
  await app.emit("agent://done", {});
  await app.release(name, null);
}

/** Give the next `send_message` a held answer called `name`. */
export async function holdNextSend(app: App, name: string): Promise<void> {
  await app.become({ send_message: deferred(name) });
}

/** Wait for the app to have sent its `n`th message. */
export async function sent(app: App, n: number): Promise<void> {
  await expect.poll(() => app.requestsFor("send_message"), { timeout: 15_000 }).toHaveLength(n);
}
