"use client";

import { useEffect, useRef, useState, useCallback } from "react";
import { GitBranch, Play, Pause, Check, Zap, Mic } from "lucide-react";

import { Reveal } from "@/components/motion";
import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

// ─── Waveform bars ────────────────────────────────────────────────────────────

function seeded(i: number, seed: number) {
  return ((Math.sin(i * 127.1 + seed * 311.7) * 43758.5453) % 1 + 1) / 2;
}

function WaveformTrack({
  label,
  seed,
  color,
  animating,
}: {
  label: string;
  seed: number;
  color: string;
  animating: boolean;
}) {
  const BARS = 56;
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const bars = ref.current?.querySelectorAll("[data-wave-bar]");
        if (!bars?.length) return;
        // One tween across every bar rather than one per bar: 56 bars ×
        // 3 tracks is 168 independent tickers otherwise, all running
        // forever on a page that also has a hero animation.
        gsap.to(bars, {
          scaleY: (i: number) =>
            animating ? 0.3 + seeded(i, seed + 99) * 0.9 : 0.5 + seeded(i, seed) * 0.7,
          duration: animating ? 0.6 : 1.6,
          ease: "sine.inOut",
          stagger: animating ? 0.004 : { each: 0.02, from: "random" },
          repeat: animating ? 0 : -1,
          yoyo: !animating,
        });
      });
      return () => mm.revert();
    },
    { scope: ref, dependencies: [animating, seed] },
  );

  return (
    <div className="mb-2">
      <p className="mb-1 truncate text-[10px] text-white/40">{label}</p>
      <div ref={ref} className="flex h-10 items-center gap-[2px]">
        {Array.from({ length: BARS }, (_, i) => (
          <div
            key={i}
            data-wave-bar
            className={`w-[3px] flex-shrink-0 origin-center rounded-full ${color}`}
            style={{ height: `${seeded(i, seed) * 80 + 10}%` }}
          />
        ))}
      </div>
    </div>
  );
}

// ─── Chat bubble ──────────────────────────────────────────────────────────────

type Role = "user" | "agent" | "tool";

interface Message {
  role: Role;
  text: string;
  tool?: string;
}

/**
 * One bubble, animating itself in as it mounts.
 *
 * The messages only ever accumulate — the sequence adds and then resets
 * the whole list — so there is no exit to animate and no need for the
 * presence bookkeeping that would buy it. `useGSAP` with no dependencies
 * runs once per mounted bubble, which is exactly the lifetime here.
 */
function ChatBubble({ msg }: { msg: Message }) {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        gsap.from(ref.current, {
          opacity: 0,
          y: 10,
          scale: msg.role === "tool" ? 1 : 0.96,
          duration: 0.45,
          ease: "back.out(1.6)",
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  if (msg.role === "tool") {
    return (
      <div
        ref={ref}
        className="mx-auto flex items-center gap-1.5 rounded-full border border-amber-500/30 bg-amber-500/10 px-3 py-1 font-mono text-[10px] text-amber-400"
      >
        <Zap className="size-2.5 shrink-0" />
        {msg.tool}
      </div>
    );
  }
  if (msg.role === "user") {
    return (
      <div
        ref={ref}
        className="ml-auto max-w-[82%] rounded-2xl rounded-br-sm bg-primary/20 px-3 py-2 text-[11px] leading-relaxed text-white ring-1 ring-primary/30"
      >
        {msg.text}
      </div>
    );
  }
  return (
    <div
      ref={ref}
      className="mr-auto max-w-[85%] rounded-2xl rounded-bl-sm bg-white/8 px-3 py-2 text-[11px] leading-relaxed text-white/80 ring-1 ring-white/10"
    >
      {msg.text}
    </div>
  );
}

// ─── Typing indicator ─────────────────────────────────────────────────────────

function TypingDots() {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        gsap.to("[data-dot]", {
          opacity: 1,
          scale: 1.15,
          duration: 0.5,
          ease: "sine.inOut",
          stagger: { each: 0.18, repeat: -1, yoyo: true },
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div
      ref={ref}
      className="mr-auto flex items-center gap-1 rounded-2xl rounded-bl-sm bg-white/8 px-3 py-2.5 ring-1 ring-white/10"
    >
      {[0, 1, 2].map((i) => (
        <span key={i} data-dot className="size-1.5 rounded-full bg-white/40 opacity-40" />
      ))}
    </div>
  );
}

// ─── Sequence ─────────────────────────────────────────────────────────────────

const SEQUENCE: Array<{ delay: number; msg?: Message; waveChange?: boolean; typing?: boolean; stopTyping?: boolean }> = [
  { delay: 800,  msg: { role: "user", text: "Isolate vocals from track 1, boost gain +3 dB, render" } },
  { delay: 900,  typing: true },
  { delay: 1800, msg: { role: "tool", tool: "stem_separate(track=1)", text: "" }, waveChange: true },
  { delay: 1600, msg: { role: "tool", tool: "gain_adjust(+3dB)", text: "" } },
  { delay: 1400, msg: { role: "tool", tool: "render_final(branch=a3f1)", text: "" }, waveChange: false },
  { delay: 900,  stopTyping: true },
  { delay: 400,  msg: { role: "agent", text: "Done. Vocals isolated, +3 dB applied. Branch a3f1 rendered. Press Ctrl+Z to A/B compare." } },
];

// ─── Main component ───────────────────────────────────────────────────────────

export function DemoFrame() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [typing, setTyping] = useState(false);
  const [waveAnimating, setWaveAnimating] = useState(false);
  const [playing, setPlaying] = useState(true);
  const [branchId] = useState("a3f1");
  const timerRefs = useRef<ReturnType<typeof setTimeout>[]>([]);
  const chatRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);

  // The playhead sweeps only while the transport says it is playing, and
  // the tween is created and killed by `playing` rather than paused —
  // a paused infinite tween still holds a slot in GSAP's ticker.
  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        if (!playing) {
          gsap.set("[data-playhead]", { width: "0%" });
          return;
        }
        gsap.fromTo(
          "[data-playhead]",
          { width: "0%" },
          { width: "100%", duration: 8, ease: "none", repeat: -1 },
        );
      });
      return () => mm.revert();
    },
    { scope: frameRef, dependencies: [playing] },
  );

  const clearTimers = useCallback(() => {
    timerRefs.current.forEach(clearTimeout);
    timerRefs.current = [];
  }, []);

  const runSequence = useCallback(() => {
    setMessages([]);
    setTyping(false);
    setWaveAnimating(false);

    let acc = 600;
    for (const step of SEQUENCE) {
      acc += step.delay;
      const t = setTimeout(() => {
        if (step.typing) setTyping(true);
        if (step.stopTyping) setTyping(false);
        if (step.waveChange !== undefined) setWaveAnimating(step.waveChange);
        if (step.msg && step.msg.text !== "") {
          setMessages((prev) => [...prev, step.msg!]);
        } else if (step.msg && step.msg.role === "tool") {
          setMessages((prev) => [...prev, step.msg!]);
        }
        if (chatRef.current) {
          chatRef.current.scrollTop = chatRef.current.scrollHeight;
        }
      }, acc);
      timerRefs.current.push(t);
    }

    // loop
    const loopDelay = acc + 3800;
    const loopTimer = setTimeout(runSequence, loopDelay);
    timerRefs.current.push(loopTimer);
  }, []); // eslint-disable-line

  useEffect(() => {
    runSequence();
    return clearTimers;
  }, [runSequence, clearTimers]);

  // scroll chat on new messages
  useEffect(() => {
    if (chatRef.current) {
      chatRef.current.scrollTop = chatRef.current.scrollHeight;
    }
  }, [messages, typing]);

  return (
    <section className="py-20 md:py-28">
      <div className="container">
        <Reveal className="mx-auto max-w-5xl">
          <div className="mb-10 text-center">
            <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
              See it in action
            </h2>
            <p className="mt-3 text-muted-foreground">
              A walkthrough of the conversational editing workflow.
            </p>
          </div>

          {/* App window */}
          <div
            ref={frameRef}
            className="overflow-hidden rounded-2xl border border-white/10 shadow-2xl"
            style={{
              background: "linear-gradient(135deg, hsl(240 10% 5%) 0%, hsl(240 10% 7%) 100%)",
            }}
          >
            {/* Title bar */}
            <div className="flex h-9 items-center gap-2 border-b border-white/8 bg-white/[0.03] px-4">
              <div className="flex gap-1.5">
                <div className="size-2.5 rounded-full bg-red-500/70" />
                <div className="size-2.5 rounded-full bg-amber-400/70" />
                <div className="size-2.5 rounded-full bg-emerald-500/70" />
              </div>
              <span className="mx-auto text-[11px] text-white/30">edytlab</span>
              <div className="flex items-center gap-1 rounded-md border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[9px] font-mono text-primary/80">
                <GitBranch className="size-2.5" />
                {branchId}
              </div>
            </div>

            {/* Two-pane layout */}
            <div className="flex h-[340px] sm:h-[380px]">
              {/* Timeline pane */}
              <div className="flex w-[62%] flex-col border-r border-white/8 p-4">
                {/* Transport bar */}
                <div className="mb-4 flex items-center gap-3">
                  <button
                    type="button"
                    aria-label={playing ? "Pause the demo" : "Play the demo"}
                    className="flex size-6 items-center justify-center rounded-full bg-primary/20 text-primary ring-1 ring-primary/30 transition-transform duration-200 hover:scale-110 active:scale-95"
                    onClick={() => setPlaying((p) => !p)}
                  >
                    {playing ? <Pause className="size-3" /> : <Play className="size-3 translate-x-[1px]" />}
                  </button>
                  <div className="flex-1 text-[10px] text-white/30">
                    <span className="font-mono">0:00</span>
                    <span className="mx-1">─</span>
                    <span className="font-mono">2:34</span>
                  </div>
                  <Mic className="size-3 text-white/20" />
                </div>

                {/* Waveform tracks */}
                <div className="flex-1 overflow-hidden">
                  <WaveformTrack
                    label="vocals.mp3"
                    seed={1}
                    color="bg-primary/70"
                    animating={waveAnimating}
                  />
                  <WaveformTrack
                    label="drums.mp3"
                    seed={7}
                    color="bg-emerald-400/60"
                    animating={false}
                  />
                  <WaveformTrack
                    label="bass.mp3"
                    seed={13}
                    color="bg-amber-400/50"
                    animating={false}
                  />
                </div>

                {/* Playhead */}
                <div className="relative mt-2 h-1 w-full overflow-hidden rounded-full bg-white/10">
                  <div data-playhead className="absolute inset-y-0 left-0 w-0 rounded-full bg-primary/60" />
                </div>
              </div>

              {/* Chat pane */}
              <div className="flex w-[38%] flex-col">
                <div className="border-b border-white/8 px-3 py-2 text-[10px] font-medium text-white/40">
                  Agent
                </div>
                <div
                  ref={chatRef}
                  className="flex flex-1 flex-col gap-2 overflow-y-auto p-3 [scrollbar-width:none]"
                >
                  {messages.map((msg, i) => (
                    <ChatBubble key={i} msg={msg} />
                  ))}
                  {typing ? <TypingDots /> : null}
                </div>

                {/* Done indicator */}
                {messages.some((m) => m.role === "agent") ? (
                  <div className="flex items-center gap-1.5 border-t border-white/8 px-3 py-2 text-[10px] text-emerald-400">
                    <Check className="size-3" />
                    Rendered · branch {branchId}
                  </div>
                ) : null}
              </div>
            </div>
          </div>
        </Reveal>
      </div>
    </section>
  );
}
