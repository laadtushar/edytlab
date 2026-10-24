import type { Metadata } from "next";

import { GoogleAnalyticsDisclosure } from "@/components/analytics/ga-disclosure";
import { LegalShell } from "@/components/landing/legal-shell";
import { GA_MEASUREMENT_ID } from "@/lib/consent";
import { pageSocial } from "@/lib/seo";

const title = "Privacy policy: your audio stays on your machine";
// Google Analytics is described only in a build that has it
// (`NEXT_PUBLIC_GA_MEASUREMENT_ID`), so the page never names a service
// the site is not running.
const description = GA_MEASUREMENT_ID
  ? "How edytlab handles your data: audio stays local, LLM keys live in the OS keychain, the app sends no telemetry, and this site uses anonymous Vercel analytics and, only if you accept, Google Analytics."
  : "How edytlab handles your data: audio stays local, LLM keys live in the OS keychain, the app sends no telemetry, and this site uses anonymous Vercel analytics.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/privacy" },
  ...pageSocial("/privacy", `${title} · edytlab`, description),
};

export default function PrivacyPage() {
  return (
    <LegalShell title="Privacy" path="/privacy" updated="2026-10-24">
      <p>
        edytlab is a local-first desktop application. This page describes what
        data the app handles and how it handles it, and what the website you
        are reading records about its visitors. The app is intentionally
        simple: it has no server of ours behind it, it sends no telemetry, and
        we do not handle your audio. The website is separate from the app and
        does use analytics, described under &ldquo;Telemetry and
        analytics&rdquo; below.
      </p>

      <h2>Audio files</h2>
      <p>
        Every audio file you load is decoded, processed, and rendered on your
        machine. Editing, mixing, and rendering all run locally in the desktop
        app, and so does transcription, once you have downloaded the speech
        model (v0.8.0 and later); it sends no audio anywhere. Stem separation
        has not shipped; it is built to run locally too once it does. We do
        not upload, copy, mirror, or
        otherwise transmit your audio. There are no &ldquo;cloud projects&rdquo;
        and there is no server-side processing.
      </p>

      <h2>LLM API keys</h2>
      <p>
        edytlab supports multiple LLM providers (Anthropic, OpenAI, Google Gemini, Groq, and OpenRouter,
        plus Ollama for a local model, which needs no key).
        When you add an API key, it is stored in your operating system&apos;s
        secure credential store: macOS Keychain on Mac, Credential Manager on
        Windows, and the Secret Service (GNOME Keyring, KWallet or KeePassXC) on Linux; on a Linux
        desktop with no Secret Service running, keys are held in the kernel keyring until you restart your computer, and
        the app tells you so, at launch and again at Save if the service went away after the app started. Keys are never written to plain-text files and never sent
        anywhere except directly to the provider whose key it is.
      </p>

      <h2>Network traffic</h2>
      <p>
        During normal use, the app&apos;s outbound network traffic goes to the
        LLM provider you have selected. Chat requests contain your
        prompt text, agent context, and tool-call results — never your raw
        audio. A tool-call result can carry text taken from your audio: after
        you ask for a transcript, the words the speech model found go to the
        provider as text, a bounded amount for each call, and with Ollama they
        stay on your computer (see &ldquo;Transcripts&rdquo; below). When you
        open Settings, the app also asks the provider shown
        there for its list of models; that request carries your API key (the
        saved one, or one you have typed) but no prompt and no audio, and for
        Ollama it goes to the address you gave it. Beyond that, the app connects only
        where you tell it to: a plugin you install from a URL is downloaded
        from that URL, an MCP server you add is contacted at the address
        you gave it, and, from v0.8.0, the speech model is
        downloaded from Hugging Face when you click Download (below). You can inspect the full set of requests in
        your OS network tools at any time.
      </p>
      <p>
        <strong>The speech model download</strong> (in v0.8.0 and later;
        v0.7.0 and earlier have no Download button and make no such
        request). Settings &rarr; Account has a Download button for the
        speech model (Whisper base, about 139 MB). The app contacts Hugging
        Face only when you click Download. It does not download anything at
        launch, when Settings opens, or when the assistant asks; no tool and
        no part of the app starts it. The click sends GET requests to{" "}
        <code>huggingface.co</code>, one for each of three files (an
        interrupted download is continued with an HTTP Range request, so a
        retry or a resume makes more), fixed to one commit of the{" "}
        <a href="https://huggingface.co/onnx-community/whisper-base_timestamped">
          onnx-community/whisper-base_timestamped
        </a>{" "}
        repository (a copy of OpenAI&apos;s{" "}
        <a href="https://huggingface.co/openai/whisper-base">whisper-base</a>,
        Apache-2.0). Hugging Face redirects the requests to its content
        delivery network. What Hugging Face sees is what any download shows
        it: your IP address, the time, which file you asked for, and a
        User-Agent of the form <code>edytlab/&lt;version&gt;</code>, so it also
        learns that the request comes from edytlab and which version of it. It
        sees nothing about your audio, your projects, your chat or your API
        keys, none of which is part of the request. The app checks each file
        against its SHA-256 hash before keeping it, and Remove in the same
        place deletes the files again. There is no telemetry: the app reports
        nothing back to us about the download, and we run no server that sees
        it. Hugging Face handles the requests under{" "}
        <a href="https://huggingface.co/privacy">its own privacy policy</a>.
        Using the model is a separate step from downloading it: transcribing
        makes no network request of its own, and the model is only read from
        your computer.
      </p>
      <p>
        <strong>Transcripts.</strong> Transcription runs on your computer with
        the model you downloaded, and your audio is not sent anywhere. But the
        assistant needs the words to work with them, so the words the model
        finds come back to it as a tool result, like the result of any other
        tool, and go to the AI provider you chose in the chat. That is text,
        never audio, and it is bounded for each call: the words come
        back one by one with their times when they fit, and as
        sentence-length segments when they do not. With Ollama the chat stays on your computer, so the words do
        too. If you do not want what was said in a recording to reach a hosted
        provider, do not ask the assistant to transcribe it, or use Ollama.
      </p>

      <h2 id="telemetry-and-analytics">Telemetry and analytics</h2>
      <p>
        <strong>The desktop app</strong> does not send telemetry or usage
        analytics.
      </p>
      <p>
        <strong>This website</strong> uses two Vercel products, which are
        loaded by the website only and are not part of the desktop app:
      </p>
      <ul>
        <li>
          <a href="https://vercel.com/docs/analytics/privacy-policy">
            Vercel Web Analytics
          </a>{" "}
          records page views. Each data point can include the time, the page
          URL, the referrer, query parameters (filtered), geolocation, the
          device&apos;s operating system and version, the browser and version,
          and the device type (mobile, tablet or desktop). It does not use
          cookies. Instead of a cookie, a visitor is identified by a hash
          created from the incoming request. Per Vercel, that hash is valid
          for a single day, after which it is automatically reset, so visitors
          can&apos;t be tracked between different days or different websites.
        </li>
        <li>
          <a href="https://vercel.com/docs/speed-insights/privacy-policy">
            Vercel Speed Insights
          </a>{" "}
          records page performance based on the Core Web Vitals. Each data
          point can include the route and URL, network speed, browser, device
          type, device operating system, country, the web vital measured and
          the page element it is attributed to.
        </li>
      </ul>
      <p>
        Vercel states that Web Analytics does not collect personal identifiers
        that track and cross-check end users&apos; data across different
        applications or websites, and does not collect or store information
        that would enable reconstructing an end user&apos;s browsing session
        across different applications or websites or personally identifying an
        end user. It states that Speed Insights does not collect or store
        information that would enable reconstructing a browsing session across
        pages or identifying a user. We see the results only as aggregated
        statistics in the Vercel dashboard for this website.{" "}
        {GA_MEASUREMENT_ID
          ? "Apart from those two and the Google Analytics described below, we do not run Plausible, Mixpanel, Segment, or any other analytics provider."
          : "We do not run Google Analytics, Plausible, Mixpanel, Segment, or any other analytics provider."}
      </p>
      <p>
        See Vercel&apos;s{" "}
        <a href="https://vercel.com/docs/analytics/privacy-policy">
          Web Analytics privacy and compliance
        </a>{" "}
        and{" "}
        <a href="https://vercel.com/docs/speed-insights/privacy-policy">
          Speed Insights privacy and compliance
        </a>{" "}
        pages, and its{" "}
        <a href="https://vercel.com/legal/privacy-notice">Privacy Notice</a>,
        for the details.
      </p>
      {GA_MEASUREMENT_ID ? <GoogleAnalyticsDisclosure /> : null}

      <h2>Crash reports</h2>
      <p>
        Future builds may include opt-in crash reporting for stability
        debugging. If we add it, it will be off by default, surfaced in
        settings, and limited to stack traces with no user content attached.
      </p>

      <h2>Updates</h2>
      <p>
        The app does not check for updates and makes no request to find out
        whether a new release exists. New versions are published on the
        GitHub releases page and announced on this website; you download and
        install them yourself. It does not check for newer models either: the
        speech model&apos;s files are pinned to one fixed version, and only
        your click on Download fetches them.
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
