/**
 * OpenAI's ChatGPT logo, as an image (#474, Sign in with ChatGPT branding).
 *
 * The two files in `assets/chatgpt/` are OpenAI's approved Sign in with
 * ChatGPT assets, byte for byte (`chatgptLogoAssets.test.ts` checks their
 * hashes). They are never recoloured, redrawn or inlined and edited: this
 * component only picks which one to show, by the surface it sits on, and sets
 * the size the image is drawn at (the aspect ratio is the file's own, 1:1).
 *
 * - `tone="on-dark"`: the white logo, for a dark surface;
 * - `tone="on-light"`: the black logo, for a light one (the primary button's
 *   violet fill takes dark text, so it takes this).
 *
 * The logo is decorative: the words "ChatGPT" are always written beside it,
 * so it has an empty `alt` and is hidden from assistive technology.
 */

import blackLogoUrl from "../assets/chatgpt/chatgpt-logo-black.svg";
import whiteLogoUrl from "../assets/chatgpt/chatgpt-logo-white.svg";

export type ChatGptLogoTone = "on-dark" | "on-light";

export interface ChatGptLogoProps {
  tone: ChatGptLogoTone;
  /** Edge length in CSS pixels. The logo is square. Defaults to 18. */
  size?: number;
  className?: string;
}

export function ChatGptLogo({ tone, size = 18, className }: ChatGptLogoProps) {
  return (
    <img
      src={tone === "on-dark" ? whiteLogoUrl : blackLogoUrl}
      alt=""
      aria-hidden="true"
      draggable={false}
      width={size}
      height={size}
      data-testid="chatgpt-logo"
      data-logo={tone === "on-dark" ? "white" : "black"}
      className={`inline-block shrink-0 select-none ${className ?? ""}`.trim()}
    />
  );
}
