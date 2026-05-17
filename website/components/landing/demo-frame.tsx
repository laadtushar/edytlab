"use client";

import { useEffect, useRef, useState, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { GitBranch, Play, Pause, Check, Zap, Mic } from "lucide-react";

import { FadeIn } from "./fade-in";

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
  return (
    <div className="mb-2">
      <p className="mb-1 truncate text-[10px] text-white/40">{label}</p>
      <div className="flex h-10 items-center gap-[2px]">
        {Array.from({ length: BARS }, (_, i) => {
          const base = seeded(i, seed) * 80 + 10;
          const target = animating
            ? seeded(i, seed + 99) * 60 + 20
            : base;
          return (
            <motion.div
              key={i}
              className={`w-[3px] flex-shrink-0 rounded-full ${color}`}
              animate={{ height: `${target}%` }}
              transition={{
                duration: animating ? 0.6 : 1.2 + seeded(i, seed) * 0.8,
                ease: "easeInOut",
                delay: animating ? i * 0.004 : seeded(i, seed) * 0.3,
                repeat: animating ? 0 : Infinity,
                repeatType: "mirror",
              }}
            />
          );
        })}
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

function ChatBubble({ msg }: { msg: Message }) {
  if (msg.role === "tool") {
    return (
      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        className="mx-auto flex items-center gap-1.5 rounded-full border border-amber-500/30 bg-amber-500/10 px-3 py-1 text-[10px] font-mono text-amber-400"
      >
        <Zap className="size-2.5 shrink-0" />
        {msg.tool}
      </motion.div>
    );
  }
  if (msg.role === "user") {
    return (
      <motion.div
        initial={{ opacity: 0, y: 10, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ type: "spring", stiffness: 280, damping: 24 }}
        className="ml-auto max-w-[82%] rounded-2xl rounded-br-sm bg-primary/20 px-3 py-2 text-[11px] leading-relaxed text-white ring-1 ring-primary/30"
      >
        {msg.text}
      </motion.div>
    );
  }
  return (
    <motion.div
      initial={{ opacity: 0, y: 10, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ type: "spring", stiffness: 280, damping: 24 }}
      className="mr-auto max-w-[85%] rounded-2xl rounded-bl-sm bg-white/8 px-3 py-2 text-[11px] leading-relaxed text-white/80 ring-1 ring-white/10"
    >
      {msg.text}
    </motion.div>
  );
}

// ─── Typing indicator ─────────────────────────────────────────────────────────

function TypingDots() {
  return (
    <div className="mr-auto flex items-center gap-1 rounded-2xl rounded-bl-sm bg-white/8 px-3 py-2.5 ring-1 ring-white/10">
      {[0, 1, 2].map((i) => (
        <motion.span
          key={i}
          className="size-1.5 rounded-full bg-white/40"
          animate={{ opacity: [0.3, 1, 0.3], scale: [0.8, 1.1, 0.8] }}
          transition={{ duration: 1, delay: i * 0.18, repeat: Infinity }}
        />
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
        <FadeIn className="mx-auto max-w-5xl">
          <div className="mb-10 text-center">
            <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
              See it in action
            </h2>
            <p className="mt-3 text-muted-foreground">
              A walkthrough of the conversational editing workflow.
            </p>
          </div>

          {/* App window */}
          <motion.div
            className="overflow-hidden rounded-2xl border border-white/10 shadow-2xl"
            initial={{ opacity: 0, y: 32, scale: 0.97 }}
            whileInView={{ opacity: 1, y: 0, scale: 1 }}
            viewport={{ once: true, amount: 0.2 }}
            transition={{ duration: 0.7, ease: [0.21, 0.47, 0.32, 0.98] }}
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
                  <motion.button
                    className="flex size-6 items-center justify-center rounded-full bg-primary/20 text-primary ring-1 ring-primary/30"
                    whileHover={{ scale: 1.15 }}
                    whileTap={{ scale: 0.9 }}
                    onClick={() => setPlaying((p) => !p)}
                  >
                    {playing ? <Pause className="size-3" /> : <Play className="size-3 translate-x-[1px]" />}
                  </motion.button>
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
                  <motion.div
                    className="absolute inset-y-0 left-0 rounded-full bg-primary/60"
                    animate={playing ? { width: ["0%", "100%"] } : {}}
                    transition={playing ? { duration: 8, repeat: Infinity, ease: "linear" } : {}}
                  />
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
                  <AnimatePresence initial={false}>
                    {messages.map((msg, i) => (
                      <ChatBubble key={i} msg={msg} />
                    ))}
                    {typing && (
                      <motion.div
                        key="typing"
                        initial={{ opacity: 0, y: 8 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -4 }}
                      >
                        <TypingDots />
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>

                {/* Done indicator */}
                <AnimatePresence>
                  {messages.some((m) => m.role === "agent") && (
                    <motion.div
                      initial={{ opacity: 0, y: 6 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0 }}
                      className="flex items-center gap-1.5 border-t border-white/8 px-3 py-2 text-[10px] text-emerald-400"
                    >
                      <Check className="size-3" />
                      Rendered · branch {branchId}
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </div>
          </motion.div>
        </FadeIn>
      </div>
    </section>
  );
}
