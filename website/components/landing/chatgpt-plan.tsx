import Link from "next/link";

import { Reveal } from "@/components/motion";

/**
 * "Use your ChatGPT plan": the seventh way in, shown with OpenAI's approved
 * Continue-with-ChatGPT button format (white logo, public/brand/openai/).
 * The pill is a link to the user guide's "How it works" section, so it has
 * a real hover and focus state; OpenAI's look (logo plus label) is
 * unchanged. Server-rendered; Reveal only fades it in.
 */
export function ChatGptPlan() {
  return (
    <section id="chatgpt-plan" className="py-10 md:py-12">
      <div className="container">
        <Reveal className="mx-auto flex max-w-3xl flex-col items-center gap-5 text-center">
          <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">
            Use your ChatGPT plan
          </h2>
          {/* Decorative waveform accent, in the site's thin violet strokes. */}
          <svg
            aria-hidden
            viewBox="0 0 120 16"
            className="-mt-2 h-4 w-28 text-primary/70"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          >
            <path d="M2 8h8M14 8h4M22 3v10M28 5v6M34 1v14M40 4v8M46 6v4M52 2v12M58 5v6M64 3v10M70 6v4M76 1v14M82 4v8M88 6v4M94 3v10M100 5v6M106 8h4M114 8h4" />
          </svg>
          <p className="text-muted-foreground">
            Sign in with a ChatGPT Plus or Pro plan instead of an API key. Your
            audio stays on your computer; the chat goes to OpenAI.
          </p>
          <Link
            href="/docs/user-guide#chatgpt-plan"
            data-press=""
            className="btn-motion btn-lift inline-flex items-center gap-2.5 rounded-full bg-black px-5 py-2.5 text-sm font-medium text-white ring-1 ring-white/20 ring-offset-background hover:bg-neutral-900 hover:ring-white/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
          >
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src="/brand/openai/chatgpt-logo-white.svg"
              alt=""
              width={21}
              height={21}
            />
            Continue with ChatGPT
            <span className="sr-only"> (how it works)</span>
          </Link>
          <p className="text-xs text-muted-foreground">
            Preview. Plus and Pro; may not be available on workspace plans.{" "}
            <Link
              href="/docs/user-guide#chatgpt-plan"
              className="font-medium text-primary underline-offset-4 hover:underline"
            >
              How it works
            </Link>
          </p>
        </Reveal>
      </div>
    </section>
  );
}
