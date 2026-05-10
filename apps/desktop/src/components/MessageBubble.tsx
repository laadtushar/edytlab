/**
 * MessageBubble — single chat row (Studio Onyx).
 *
 * Asymmetric bubbles: the user side is a soft amber-tinted card that
 * reads as outgoing intent; the assistant side is a flat onyx card
 * with no border so the transcript reads more like a conversation
 * than a form. Streaming bubbles reuse this with `pending` so the
 * caret is visible while text is still arriving.
 */

import type { ChatRole } from "../hooks/useAgentStream";

export interface MessageBubbleProps {
  role: ChatRole;
  text: string;
  pending?: boolean;
}

export function MessageBubble({ role, text, pending }: MessageBubbleProps) {
  const isUser = role === "user";
  return (
    <div
      data-testid="message-bubble"
      data-role={role}
      className={
        "flex w-full app-fade-in " +
        (isUser ? "justify-end" : "justify-start")
      }
    >
      <div
        className={
          "max-w-[85%] whitespace-pre-wrap break-words text-sm leading-relaxed " +
          (isUser
            ? "rounded-2xl rounded-br-sm bg-[var(--accent-soft)] border border-[var(--accent)]/25 px-3.5 py-2 text-[var(--text)]"
            : "rounded-2xl rounded-bl-sm bg-[var(--surface-elev)] px-3.5 py-2 text-[var(--text)]")
        }
      >
        {text}
        {pending ? (
          <span
            data-testid="caret"
            className="ml-0.5 inline-block w-1.5 animate-pulse text-[var(--accent)]"
          >
            ▍
          </span>
        ) : null}
      </div>
    </div>
  );
}
