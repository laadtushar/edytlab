import Link from "next/link";

import { Reveal } from "@/components/motion";

/**
 * "Use your ChatGPT plan": the seventh way in, shown with OpenAI's approved
 * Continue-with-ChatGPT button format (white logo, public/brand/openai/).
 * The button is a picture of the app's button, not a control, so it is not
 * a link or a button here. Server-rendered; Reveal only fades it in.
 */
export function ChatGptPlan() {
  return (
    <section id="chatgpt-plan" className="py-12 md:py-20">
      <div className="container">
        <Reveal className="mx-auto flex max-w-3xl flex-col items-center gap-5 text-center">
          <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">
            Use your ChatGPT plan
          </h2>
          <p className="text-muted-foreground">
            Sign in with a ChatGPT Plus or Pro plan instead of an API key. Your
            audio stays on your computer; the chat goes to OpenAI.
          </p>
          <div
            role="img"
            aria-label="Continue with ChatGPT, as it appears in the app"
            className="inline-flex items-center gap-2.5 rounded-full bg-black px-5 py-2.5 text-sm font-medium text-white ring-1 ring-white/20"
          >
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src="/brand/openai/chatgpt-logo-white.svg"
              alt=""
              width={21}
              height={21}
            />
            Continue with ChatGPT
          </div>
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
