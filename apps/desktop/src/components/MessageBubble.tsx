/**
 * MessageBubble — single chat row.
 *
 * Rendering is deliberately simple: a left-aligned card for assistant
 * turns, a right-aligned card for user turns. The streaming bubble
 * (rendered by Chat) reuses this component with `pending` so the caret
 * is visible while text is still arriving.
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
        "flex w-full " + (isUser ? "justify-end" : "justify-start")
      }
    >
      <div
        className={
          "max-w-[85%] rounded-lg px-3 py-2 text-sm whitespace-pre-wrap break-words " +
          (isUser
            ? "bg-blue-600 text-white"
            : "bg-neutral-800 text-neutral-100")
        }
      >
        {text}
        {pending ? (
          <span
            data-testid="caret"
            className="ml-0.5 inline-block w-2 animate-pulse"
          >
            ▍
          </span>
        ) : null}
      </div>
    </div>
  );
}
