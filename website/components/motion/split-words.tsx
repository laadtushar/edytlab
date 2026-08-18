"use client";

import { useRef } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

interface SplitWordsProps {
  text: string;
  className?: string;
  /** Class on each word — how the gradient gets onto the headline. */
  wordClassName?: string;
  delay?: number;
  each?: number;
  /** Fire on scroll instead of on mount. */
  onScroll?: boolean;
}

/**
 * A headline that assembles itself a word at a time.
 *
 * Split by word rather than by character. Per-character reveals look
 * impressive on a three-word logotype and become unreadable on a
 * sentence — the eye tries to read letters as they land. Words keep the
 * effect and keep the sentence.
 *
 * The whole string stays in the DOM as text: each word is a `span` with
 * the real characters inside, so a screen reader reads the sentence and
 * a search engine indexes it. Nothing here is drawn.
 */
export function SplitWords({
  text,
  className,
  wordClassName,
  delay = 0,
  each = 0.055,
  onScroll = false,
}: SplitWordsProps) {
  const ref = useRef<HTMLSpanElement>(null);
  const words = text.split(" ");

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const spans = ref.current?.querySelectorAll<HTMLElement>("[data-word]");
        if (!spans?.length) return;
        gsap.from(spans, {
          opacity: 0,
          yPercent: 120,
          rotateX: -55,
          duration: 0.85,
          delay,
          ease: "power4.out",
          stagger: each,
          ...(onScroll
            ? { scrollTrigger: { trigger: ref.current, start: "top 85%", once: true } }
            : {}),
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <span ref={ref} className={className} style={{ perspective: 800 }}>
      {words.map((w, i) => (
        <span key={`${w}-${i}`}>
          {/* The outer span clips, so a word rising from `yPercent: 120`
              comes up from behind the line above rather than overlapping
              it. */}
          <span className="inline-block overflow-hidden py-[0.08em] align-bottom">
            <span data-word className={`inline-block ${wordClassName ?? ""}`}>
              {w}
            </span>
          </span>
          {/* A real space as a text node, outside the inline-blocks. An
              `&nbsp;` in its own `inline-block` does not collapse the way
              a word space does, and the headline ends up visibly
              loose-set. This also keeps the sentence copy-pasteable. */}
          {i < words.length - 1 ? " " : null}
        </span>
      ))}
    </span>
  );
}
