/**
 * MessageBubble — single chat row (Studio Onyx).
 *
 * Asymmetric bubbles: the user side is a soft amber-tinted card that
 * reads as outgoing intent; the assistant side is a flat onyx card
 * with no border so the transcript reads more like a conversation
 * than a form. Streaming bubbles reuse this with `pending` so the
 * caret is visible while text is still arriving.
 *
 * Action chips: assistant bubbles can carry a row of one-tap buttons
 * below the text. Each chip resubmits its `prompt` through the
 * agent — see `useAgentStream.deriveChips` for how they're populated
 * and `docs/specs/agentic-chat-ui.md` for the design rationale.
 */

import type { ChatRole, Chip, ChipIcon } from "../hooks/useAgentStream";

export interface MessageBubbleProps {
  role: ChatRole;
  text: string;
  pending?: boolean;
  /** Optional action chips. Only honoured for `role === "assistant"`. */
  chips?: Chip[];
  /** Click handler for a chip. Receives the chip's `prompt`. */
  onChipClick?: (prompt: string) => void;
}

export function MessageBubble({
  role,
  text,
  pending,
  chips,
  onChipClick,
}: MessageBubbleProps) {
  const isUser = role === "user";
  // The bubble is `whitespace-pre-wrap`, which is right for the middle of
  // a message and wrong at its edges. Models routinely open a reply with
  // newlines — more so once a thinking block has been stripped out — and
  // every one of them was rendered, so a bubble grew tall and blank with
  // the caret stranded at the bottom. Trimming the ends is purely
  // presentational: nobody means to begin a sentence with three blank
  // lines, and interior formatting is untouched.
  const body = text.replace(/^\s+/, "").replace(/\s+$/, "");
  const showChips =
    !isUser && !pending && chips && chips.length > 0 && !!onChipClick;
  return (
    <div
      data-testid="message-bubble"
      data-role={role}
      className={
        "flex w-full flex-col gap-1.5 app-fade-in " +
        (isUser ? "items-end" : "items-start")
      }
    >
      <div
        className={
          "max-w-[85%] whitespace-pre-wrap break-words text-sm leading-relaxed " +
          // An in-flight bubble with nothing in it yet should be the size
          // of the caret, not the size of a paragraph.
          (body.length === 0 && pending ? "inline-flex items-center " : "") +
          (isUser
            ? "rounded-2xl rounded-br-sm bg-[var(--accent-soft)] border border-[var(--accent)]/25 px-3.5 py-2 text-[var(--text)]"
            : "rounded-2xl rounded-bl-sm bg-[var(--surface-elev)] px-3.5 py-2 text-[var(--text)]")
        }
      >
        {body}
        {pending ? (
          <span
            data-testid="caret"
            aria-hidden="true"
            className="ml-0.5 inline-block h-[1.05em] w-[2px] translate-y-[0.18em] animate-pulse rounded-[1px] bg-[var(--accent)] align-baseline"
          />
        ) : null}
      </div>
      {showChips ? (
        <div
          data-testid="message-chips"
          className="flex max-w-[85%] flex-wrap gap-1.5"
        >
          {chips!.map((c) => (
            <button
              key={c.id}
              type="button"
              data-testid="message-chip"
              data-chip-id={c.id}
              onClick={() => onChipClick!(c.prompt)}
              className="
                inline-flex items-center gap-1.5
                rounded-full border border-[var(--border-strong)]
                bg-[var(--surface-elev)]
                px-2.5 py-1 text-xs text-[var(--text-dim)]
                transition
                hover:border-[var(--accent)]/45 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
              "
            >
              <ChipGlyph icon={c.icon} />
              <span>{c.label}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function ChipGlyph({ icon }: { icon: ChipIcon }) {
  const common = {
    width: 11,
    height: 11,
    viewBox: "0 0 11 11",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.6,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
  };
  switch (icon) {
    case "play":
      // Filled triangle reads as "preview / play" in audio context.
      return (
        <svg {...common} fill="currentColor" stroke="none">
          <path d="M2 1.5v8l7-4z" />
        </svg>
      );
    case "save":
      return (
        <svg {...common}>
          <path d="M2 2v7h7V3.5L7.5 2H2z" />
          <path d="M3.5 2v2.5h3V2" />
        </svg>
      );
    case "wand":
      return (
        <svg {...common}>
          <path d="M2 9L9 2" />
          <path d="M7.5 1.5l1 1M1.5 7.5l1 1" />
        </svg>
      );
    case "scissors":
      return (
        <svg {...common}>
          <circle cx="3" cy="3" r="1.5" />
          <circle cx="3" cy="8" r="1.5" />
          <path d="M4.2 4l5 5M4.2 7l5-5" />
        </svg>
      );
    case "undo":
      return (
        <svg {...common}>
          <path d="M3 5.5h4a2 2 0 1 1 0 4H4" />
          <path d="M5 3.5L3 5.5l2 2" />
        </svg>
      );
  }
}
