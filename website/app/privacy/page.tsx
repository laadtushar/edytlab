import type { Metadata } from "next";

import { LegalShell } from "@/components/landing/legal-shell";

export const metadata: Metadata = {
  title: "Privacy",
  description:
    "How edytlab handles your data. Audio stays local, LLM keys live in the OS keychain, no telemetry.",
};

export default function PrivacyPage() {
  return (
    <LegalShell title="Privacy" updated="2026-05-08">
      <p>
        edytlab is a local-first desktop application. This page describes what
        data the app handles and how it handles it. It is intentionally short
        because the app is intentionally simple: we do not operate servers, we
        do not collect telemetry, and we do not handle your audio.
      </p>

      <h2>Audio files</h2>
      <p>
        Every audio file you load is decoded, processed, and rendered on your
        machine. Stem separation, transcription, mixing, and rendering all run
        locally in the desktop app. We do not upload, copy, mirror, or
        otherwise transmit your audio. There are no &ldquo;cloud projects&rdquo;
        and there is no server-side processing.
      </p>

      <h2>LLM API keys</h2>
      <p>
        edytlab supports multiple LLM providers (Anthropic, OpenRouter, OpenAI).
        When you add an API key, it is stored in your operating system&apos;s
        secure credential store: macOS Keychain on Mac and Credential Manager on
        Windows. Keys are never written to plain-text files and never sent
        anywhere except directly to the provider whose key it is.
      </p>

      <h2>Network traffic</h2>
      <p>
        The only outbound network traffic the app makes during normal use is
        chat requests to the LLM provider you have selected. These requests
        contain your prompt text, agent context, and tool-call results — never
        your raw audio. You can inspect the full set of requests in your OS
        network tools at any time.
      </p>

      <h2>Telemetry and analytics</h2>
      <p>
        There is none. The desktop app does not send usage analytics. The
        marketing website you are reading does not run Google Analytics,
        Plausible, Mixpanel, Segment, or any other analytics or tracking
        provider, and sets no analytics cookies.
      </p>

      <h2>Crash reports</h2>
      <p>
        Future builds may include opt-in crash reporting for stability
        debugging. If we add it, it will be off by default, surfaced in
        settings, and limited to stack traces with no user content attached.
      </p>

      <h2>Updates</h2>
      <p>
        The app may check for new releases. This check fetches a small JSON
        manifest from the public release host. No identifying information is
        sent.
      </p>

      <h2>Contact</h2>
      <p>
        Questions about this policy can be raised as an issue in the public{" "}
        <a href="https://github.com/laadtushar/edytlab/issues">
          GitHub repository
        </a>
        .
      </p>
    </LegalShell>
  );
}
