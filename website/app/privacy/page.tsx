import type { Metadata } from "next";

import { LegalShell } from "@/components/landing/legal-shell";
import { pageSocial } from "@/lib/seo";

const title = "Privacy policy: your audio stays on your machine";
const description =
  "How edytlab handles your data: audio stays local, LLM keys live in the OS keychain, the app sends no telemetry, and this site uses anonymous Vercel analytics.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/privacy" },
  ...pageSocial("/privacy", `${title} · edytlab`, description),
};

export default function PrivacyPage() {
  return (
    <LegalShell title="Privacy" updated="2026-10-10">
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
        app, and stem separation and transcription are built to run locally
        too once they ship. We do not upload, copy, mirror, or
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
        desktop with no Secret Service running, keys are held in the kernel keyring until you restart, and
        the app tells you so. Keys are never written to plain-text files and never sent
        anywhere except directly to the provider whose key it is.
      </p>

      <h2>Network traffic</h2>
      <p>
        The only outbound network traffic the app makes during normal use is
        chat requests to the LLM provider you have selected. These requests
        contain your prompt text, agent context, and tool-call results — never
        your raw audio. Beyond that, the app connects only where you tell it
        to: a plugin you install from a URL is downloaded from that URL, and an
        MCP server you add is contacted at the address you gave it. You can
        inspect the full set of requests in your OS network tools at any time.
      </p>

      <h2>Telemetry and analytics</h2>
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
        statistics in the Vercel dashboard for this website. We do not run
        Google Analytics, Plausible, Mixpanel, Segment, or any other analytics
        provider.
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
        install them yourself.
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
