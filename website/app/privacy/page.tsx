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
  ? "How edytlab handles your data: audio stays local, LLM keys and a ChatGPT sign-in token live in the OS keychain, the app sends no telemetry, and this site uses anonymous Vercel analytics and, only if you accept, Google Analytics."
  : "How edytlab handles your data: audio stays local, LLM keys and a ChatGPT sign-in token live in the OS keychain, the app sends no telemetry, and this site uses anonymous Vercel analytics.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/privacy" },
  ...pageSocial("/privacy", `${title} · edytlab`, description),
};

export default function PrivacyPage() {
  return (
    <LegalShell title="Privacy" path="/privacy" updated="2026-10-25">
      <p>
        edytlab is a local-first desktop application. It has no server of ours
        behind it, it sends no telemetry, and we do not handle your audio. The
        website is separate from the app and does use analytics, described
        under &ldquo;Telemetry and analytics&rdquo; below.
      </p>

      <h2>Audio files</h2>
      <p>
        Every audio file you load is decoded, processed, and rendered on your
        machine. Editing, mixing, and rendering run locally, and so does
        transcription, once you have downloaded the speech model (v0.8.0 and
        later); it sends no audio anywhere. Stem separation has not shipped; it
        is built to run locally too once it does. We do not upload, copy,
        mirror, or otherwise transmit your audio. There are no &ldquo;cloud
        projects&rdquo; and there is no server-side processing.
      </p>

      <h2>LLM API keys</h2>
      <p>
        edytlab supports six API providers (Anthropic, OpenAI, Google Gemini, Groq, and OpenRouter,
        plus Ollama for a local model, which needs no key) and, from v0.9.0, your ChatGPT plan,
        which has no API key (see &ldquo;Signing in with ChatGPT&rdquo; below).
        An API key you add is stored in your operating system&apos;s secure
        credential store: macOS Keychain, Credential Manager on Windows, or the
        Secret Service (GNOME Keyring, KWallet or KeePassXC) on Linux. On a
        Linux desktop with no Secret Service running, keys are held in the
        kernel keyring until you restart your computer, and the app tells you
        so, at launch and again at Save if the service went away after the app
        started. Keys are never written to plain-text files and never sent
        anywhere except directly to the provider whose key it is.
      </p>

      <h2>Network traffic</h2>
      <p>
        During normal use, the app&apos;s outbound network traffic goes to the
        LLM provider you have selected. Chat requests contain your
        prompt text, agent context, and tool-call results — never your raw
        audio. A tool-call result can carry text taken from your audio, the
        words of a transcript (see &ldquo;Transcripts&rdquo; below). When you
        open Settings, the app also asks the provider shown there for its list
        of models; that request carries your API key (the saved one, or one you
        have typed) but no prompt and no audio, and for Ollama it goes to the
        address you gave it. With the ChatGPT plan the
        provider is OpenAI, the requests carry your ChatGPT access token in
        place of a key, and signing in, staying signed in and signing out also
        contact <code>auth.openai.com</code> (see &ldquo;Signing in with
        ChatGPT&rdquo; below). Beyond that, the app connects only where you tell
        it to: a plugin you install from a URL is downloaded from that URL, an
        MCP server you add is contacted at the address you gave it, and, from
        v0.8.0, the speech model is downloaded from Hugging Face when you click
        Download (below). Your OS network tools show every request.
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
        Transcribing makes no network request of its own; the model is only
        read from your computer.
      </p>
      <p>
        <strong>Transcripts.</strong> Transcription runs on your computer with
        the model you downloaded, and your audio is not sent anywhere. But the
        words the model finds come back to the assistant as a tool result, like
        any other tool&apos;s, and go to the AI provider you chose in the chat.
        That is text, never audio, bounded for each call: word by word with
        their times when they fit, and as sentence-length segments when they
        do not. With Ollama the chat stays on your computer, so the words do
        too. If you do not want what was said in a recording to reach a hosted
        provider, do not ask the assistant to transcribe it, or use Ollama.
      </p>

      <h2 id="signing-in-with-chatgpt">Signing in with ChatGPT</h2>
      <p>
        In v0.9.0 and later, Settings &rarr; Account can run the assistant on
        your ChatGPT plan instead of an API key, through OpenAI&apos;s{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source">
          Sign in with ChatGPT
        </a>{" "}
        for open-source apps, which OpenAI documents as a preview. v0.8.1 and
        earlier have no ChatGPT sign-in and make none of the requests below.
        Until you click Continue with ChatGPT, v0.9.0 makes none of them
        either. Every request goes from your computer to OpenAI, and nothing
        is reported to us.
      </p>
      <p>
        <strong>Signing in.</strong> Continue with ChatGPT opens your default
        browser at <code>auth.openai.com</code>, where you sign in on
        OpenAI&apos;s page and see what edytlab is asking for. The address
        carries edytlab&apos;s client ID (see below), this installation&apos;s
        random host ID, the scopes <code>openid</code>, <code>profile</code>,{" "}
        <code>email</code>, <code>offline_access</code>,{" "}
        <code>resource.invoke</code> and <code>chatgpt.tokens.use.direct</code>
        , and a return address on <code>127.0.0.1</code>. The first time it
        also carries the name <code>edytlab</code>; on a later sign-in to the
        same account it carries your email address as a hint, so that OpenAI
        can pre-select the account. When you finish, your browser goes to the
        return address: a listener that edytlab opens on your own computer
        for up to five minutes, so that step does not leave the machine.
        edytlab then sends the code to <code>auth.openai.com</code> to
        exchange it for tokens, and checks the signed identity token that
        comes back against OpenAI&apos;s published keys. Per{" "}
        <a href="https://developers.openai.com/siwc/quickstart">
          OpenAI&apos;s documentation
        </a>
        , using a ChatGPT plan &ldquo;doesn&apos;t grant access to the
        user&apos;s API key or conversations&rdquo;.
      </p>
      <p>
        <strong>Staying signed in.</strong> An access token is valid for one
        hour and a refresh token for 30 days, which each renewal extends (
        <a href="https://developers.openai.com/siwc/token-sharing-open-source/token-reference">
          OpenAI&apos;s token reference
        </a>
        ). When the assistant or the model list needs an access token that has
        expired or is about to, edytlab sends the refresh token to{" "}
        <code>auth.openai.com</code> and receives a new access token and a
        replacement refresh token, which it saves in place of the old one. It
        also reads OpenAI&apos;s public sign-in configuration and signing keys
        from the same host when it needs them. None of these requests carries
        your chat.
      </p>
      <p>
        <strong>What a turn sends.</strong> With the plan active, the
        assistant&apos;s requests go to <code>api.openai.com</code> (
        <code>POST /v1/responses</code>) with your access token as the bearer,
        in place of an API key. They carry what any provider&apos;s request
        carries: your prompt text, agent context and tool-call results, as
        text and never your raw audio, and the &ldquo;Transcripts&rdquo;
        paragraph above applies to them as it does to any hosted provider.
        OpenAI&apos;s rules for this flow have the request set{" "}
        <code>store</code> to <code>false</code> and carry the conversation
        each time (
        <a href="https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations">
          preview limitations
        </a>
        ); what OpenAI does with the text is covered by its own terms and
        privacy policy. The same bearer asks{" "}
        <code>api.openai.com</code> for the models your account can use (
        <code>GET /v1/models</code>), when the assistant is set up on the plan
        and when Settings lists them, so that it can offer them and their
        reasoning levels; that request has no prompt and no audio. OpenAI
        sees your IP address and the text, as any provider does, and because
        the bearer belongs to your ChatGPT account it can tie the requests to
        that account, to edytlab&apos;s registration and to this
        installation&apos;s host ID, which is how it counts plan usage for each
        app (
        <a href="https://developers.openai.com/siwc/token-sharing-open-source">
          OpenAI&apos;s overview
        </a>
        ). edytlab does not retry a failed plan request on another provider,
        or with an API key.
      </p>
      <p>
        <strong>What edytlab keeps.</strong> Only the refresh token is a
        secret that edytlab keeps. It goes in your operating system&apos;s
        credential store, in the slot <code>chatgpt_refresh_token</code>, and
        is replaced at every renewal. On Linux with no Secret Service running
        it is held in the kernel keyring until you restart, and on Windows a
        token too long for Credential Manager is held in memory only; in
        either case Settings says so, and you sign in again after a restart.
        The access token is held in memory and never written anywhere, and the
        identity token is not kept once it has been checked. The rest is in a
        file, <code>registration.json</code>, in a <code>chatgpt</code> folder
        inside the app&apos;s config folder (<code>app.edytlab.desktop</code>{" "}
        in your user config directory), readable by your user only on macOS and
        Linux: the host ID (a random <code>urn:uuid:</code> value, not derived
        from you, that OpenAI documents as not being a credential), the client
        ID OpenAI issued for edytlab, the account&apos;s identifier and email
        address, the scopes granted, the time it was saved and a few claims
        from the sign-in&apos;s identity token that later ones are checked
        against. The email address is shown in Settings as &ldquo;Signed in
        as&rdquo;, and is sent back to OpenAI as the hint above; edytlab does
        not write it to its logs.
      </p>
      <p>
        <strong>Signing out.</strong> Sign out sends the refresh token to
        OpenAI&apos;s revocation endpoint, named in its sign-in
        configuration, and then deletes it from the credential store. If
        OpenAI&apos;s answer cannot be confirmed (you are offline, say), the
        token is still deleted from your computer, and Settings tells you that
        edytlab could not confirm that OpenAI ended the sign-in. Signing out
        keeps <code>registration.json</code>, including the email address, so
        that the next sign-in reuses the same registration instead of adding
        another connection to your ChatGPT settings. Start over signs out and
        then forgets the registration (the client ID, the account and the
        email), keeping only the host ID, so the next sign-in registers edytlab
        again. When you use another account, the first account&apos;s refresh
        token is revoked and removed once the second sign-in has succeeded.
      </p>
      <p>
        <strong>Signing out does not disconnect edytlab in ChatGPT.</strong>{" "}
        OpenAI&apos;s Help Center says it plainly:{" "}
        &ldquo;Signing out of the app ends your session there, but does not
        disconnect it from ChatGPT or stop its authorized use of your
        plan.&rdquo; To disconnect edytlab, open ChatGPT, go to Settings
        &rarr; Security and login, choose Sign in with ChatGPT under Login
        connections, select edytlab and choose Disconnect, where ChatGPT shows
        those controls. That also removes an entry that Start over left behind.
        Disconnecting does not reverse usage already recorded, and OpenAI does
        not tell edytlab when you disconnect it: the next time edytlab uses
        the session it finds out, and asks you to sign in again (
        <a href="https://help.openai.com/en/articles/20001542-using-your-chatgpt-plan-in-other-apps-and-sites">
          Help Center
        </a>
        ,{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery">
          errors and recovery
        </a>
        ). The Manage usage button opens{" "}
        <code>chatgpt.com/settings/usage</code> in your browser; the app itself
        sends nothing there.
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
