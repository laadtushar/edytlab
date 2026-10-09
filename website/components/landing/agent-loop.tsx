"use client";

/**
 * The agent loop, drawn: you → the agent → tools → the engine → a render,
 * with the result going back to the agent until the job is done.
 *
 * It sits under "the agent layer in between" because that sentence is
 * this picture. Everything it shows is how the app works: the model
 * plans and calls tools by name, the tools edit through the Rust engine
 * on your machine, and what goes back to the model is the tool's result
 * — text, not audio. The dashed box is the machine the audio never
 * leaves.
 *
 * Two layouts of the same diagram — across for a wide screen, down for a
 * phone — because one SVG scaled to 390px would set its labels at five
 * pixels. Only the visible one animates.
 *
 * Motion, with the finished drawing in the markup:
 * - the connectors are drawn in order (DrawSVG), the boxes pop,
 * - then dots travel the wires (MotionPath) — a request in, a tool
 *   call, the result back round the loop, a second call, the render —
 *   on a loop that only runs while the diagram is on screen.
 */

import { useRef } from "react";

import {
  gsap,
  useGSAP,
  motionOk,
  NO_PREFERENCE,
  REVEAL_START,
  undash,
  whileVisible,
} from "@/lib/gsap";
import "@/lib/gsap-svg";
import { cn } from "@/lib/utils";

type NodeId = "you" | "agent" | "tools" | "engine" | "render";
type EdgeId = "ask" | "call" | "edit" | "result" | "done";

interface Layout {
  viewBox: string;
  /** Node centres. Every box is NODE_W × NODE_H. */
  nodes: Record<NodeId, [number, number]>;
  edges: Record<EdgeId, string>;
  /** Where each edge's label sits. */
  labels: Record<EdgeId, [number, number]>;
  /** The dashed "your machine" box: x, y, w, h, and its label's anchor. */
  local: [number, number, number, number];
  localLabel: [number, number];
}

const NODE_W = 132;
const NODE_H = 56;

const NODES: Record<NodeId, { title: string; sub: string }> = {
  you: { title: "You", sub: "a sentence" },
  agent: { title: "Agent", sub: "your LLM plans" },
  tools: { title: "Tools", sub: "called by name" },
  engine: { title: "Engine", sub: "Rust DSP" },
  render: { title: "Render", sub: "WAV · FLAC · MP3" },
};

const EDGE_LABELS: Record<EdgeId, string> = {
  ask: "prompt",
  call: "tool call",
  edit: "edit",
  result: "result — text, not audio",
  done: "done",
};

const WIDE: Layout = {
  viewBox: "0 0 940 260",
  nodes: { you: [76, 100], agent: [266, 100], tools: [470, 100], engine: [670, 100], render: [864, 100] },
  edges: {
    ask: "M142,100 L200,100",
    call: "M332,100 L404,100",
    edit: "M536,100 L604,100",
    result: "M670,128 C670,214 266,214 266,128",
    done: "M736,100 L798,100",
  },
  labels: {
    ask: [171, 88],
    call: [368, 88],
    edit: [570, 88],
    result: [468, 226],
    done: [767, 88],
  },
  local: [388, 44, 548, 196],
  localLabel: [924, 34],
};

const TALL: Layout = {
  viewBox: "0 0 340 600",
  nodes: { you: [130, 40], agent: [130, 150], tools: [130, 290], engine: [130, 410], render: [130, 540] },
  edges: {
    ask: "M130,68 L130,122",
    call: "M130,178 L130,262",
    edit: "M130,318 L130,382",
    result: "M196,410 C300,410 300,150 196,150",
    done: "M130,438 L130,512",
  },
  labels: {
    ask: [142, 99],
    call: [142, 224],
    edit: [142, 354],
    result: [318, 280],
    done: [142, 479],
  },
  local: [36, 236, 296, 346],
  localLabel: [324, 598],
};

function Diagram({ layout, className }: { layout: Layout; className?: string }) {
  const tall = layout === TALL;
  return (
    <svg
      viewBox={layout.viewBox}
      className={cn("h-auto w-full overflow-visible", className)}
      role="img"
      aria-label="The agent loop: your sentence goes to the agent, which calls tools by name; the tools edit the audio through the Rust engine on your machine; the result goes back to the agent as text, and when the job is done it renders WAV, FLAC or MP3."
    >
      {/* Your machine — where the audio is, and stays. */}
      <rect
        data-local
        x={layout.local[0]}
        y={layout.local[1]}
        width={layout.local[2]}
        height={layout.local[3]}
        rx="16"
        fill="none"
        strokeDasharray="5 6"
        className="stroke-primary/35"
        strokeWidth="1.25"
      />
      <text
        x={layout.localLabel[0]}
        y={layout.localLabel[1]}
        textAnchor="end"
        className="fill-primary/80 font-mono text-[11px] uppercase tracking-widest"
      >
        on your machine
      </text>

      {(Object.keys(layout.edges) as EdgeId[]).map((id) => (
        <g key={id}>
          <path
            data-edge={id}
            d={layout.edges[id]}
            fill="none"
            className={id === "result" ? "stroke-fuchsia-400/60" : "stroke-primary/60"}
            strokeWidth="1.5"
            strokeLinecap="round"
          />
          <text
            data-edge-label
            x={layout.labels[id][0]}
            y={layout.labels[id][1]}
            textAnchor={tall && id !== "result" ? "start" : "middle"}
            className="fill-muted-foreground text-[11px]"
            transform={
              tall && id === "result"
                ? `rotate(90 ${layout.labels[id][0]} ${layout.labels[id][1]})`
                : undefined
            }
          >
            {EDGE_LABELS[id]}
          </text>
        </g>
      ))}

      {(Object.keys(NODES) as NodeId[]).map((id) => {
        const [cx, cy] = layout.nodes[id];
        return (
          <g key={id} data-node={id}>
            <rect
              x={cx - NODE_W / 2}
              y={cy - NODE_H / 2}
              width={NODE_W}
              height={NODE_H}
              rx="12"
              className={cn(
                "fill-card stroke-border",
                id === "agent" && "stroke-primary/60",
              )}
              strokeWidth="1.25"
            />
            <text
              x={cx}
              y={cy - 4}
              textAnchor="middle"
              className="fill-foreground text-[15px] font-semibold"
            >
              {NODES[id].title}
            </text>
            <text x={cx} y={cy + 15} textAnchor="middle" className="fill-muted-foreground text-[11px]">
              {NODES[id].sub}
            </text>
          </g>
        );
      })}

      {/* The travelling dots. Decoration that only exists in motion, so
          they are transparent until the script moves them. */}
      {(Object.keys(layout.edges) as EdgeId[]).map((id) => (
        <circle
          key={id}
          data-dot={id}
          r="4.5"
          cx="0"
          cy="0"
          opacity="0"
          className={id === "result" ? "fill-fuchsia-300" : "fill-primary"}
        />
      ))}
    </svg>
  );
}

function useLoopMotion(query: string) {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(`${NO_PREFERENCE} and ${query}`, (context) => {
        const root = ref.current;
        if (!root) return;
        const q = gsap.utils.selector(root);

        // 1 · Build: boxes pop in order, wires draw between them.
        const order: Array<[NodeId, EdgeId | null]> = [
          ["you", "ask"],
          ["agent", "call"],
          ["tools", "edit"],
          ["engine", "result"],
          ["render", "done"],
        ];
        const build = gsap.timeline({
          scrollTrigger: { trigger: root, start: REVEAL_START, once: true },
        });
        build.from(q("[data-local]"), { opacity: 0, duration: 0.6 });
        order.forEach(([node], i) => {
          build.from(
            q(`[data-node='${node}']`),
            { opacity: 0, scale: 0.85, transformOrigin: "50% 50%", duration: 0.45, ease: "back.out(2)" },
            i === 0 ? "<" : "-=0.25",
          );
        });
        build
          .fromTo(
            q("[data-edge]"),
            { drawSVG: "0%" },
            {
              drawSVG: "100%",
              duration: 0.6,
              stagger: 0.15,
              ease: "power2.inOut",
              onComplete: undash(q("[data-edge]")),
            },
            "-=0.6",
          )
          .from(q("[data-edge-label]"), { opacity: 0, duration: 0.4, stagger: 0.08 }, "-=0.4");

        // 2 · Flow: one pass round the loop, repeated while on screen.
        const travel = (edge: EdgeId, duration: number) =>
          gsap
            .timeline()
            .set(q(`[data-dot='${edge}']`), { opacity: 1 })
            .to(q(`[data-dot='${edge}']`), {
              motionPath: { path: root.querySelector<SVGPathElement>(`[data-edge='${edge}']`)! },
              duration,
              ease: "power1.inOut",
            })
            .set(q(`[data-dot='${edge}']`), { opacity: 0 });
        // A box that receives something gives a small beat.
        const pulse = (node: NodeId) =>
          gsap.fromTo(
            q(`[data-node='${node}']`),
            { scale: 1 },
            {
              scale: 1.06,
              transformOrigin: "50% 50%",
              duration: 0.16,
              ease: "power2.out",
              yoyo: true,
              repeat: 1,
            },
          );

        const flow = gsap
          .timeline({ repeat: -1, repeatDelay: 1 })
          .add(travel("ask", 0.7))
          .add(pulse("agent"))
          .add(travel("call", 0.55))
          .add(travel("edit", 0.5))
          .add(pulse("engine"))
          .add(travel("result", 1.1))
          .add(pulse("agent"))
          .add(travel("call", 0.55))
          .add(travel("edit", 0.5))
          .add(travel("done", 0.55))
          .add(pulse("render"));

        // Starts once the drawing is built, and only runs while visible.
        // `context.add` so the visibility trigger, made later than the
        // rest, is still reverted with them on unmount.
        flow.pause();
        build.eventCallback("onComplete", () => {
          context.add(() => whileVisible(flow, root));
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return ref;
}

export function AgentLoop({ className }: { className?: string }) {
  const wide = useLoopMotion("(min-width: 768px)");
  const tall = useLoopMotion("(max-width: 767px)");

  return (
    <div className={cn("mx-auto mb-14 max-w-4xl", className)}>
      <div ref={wide} className="hidden md:block">
        <Diagram layout={WIDE} />
      </div>
      <div ref={tall} className="mx-auto max-w-[340px] md:hidden">
        <Diagram layout={TALL} />
      </div>
    </div>
  );
}
