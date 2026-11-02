import type { Metadata } from "next";

import { GoogleAnalyticsDisclosure } from "@/components/analytics/ga-disclosure";
import { LegalShell } from "@/components/landing/legal-shell";
import { GA_MEASUREMENT_ID } from "@/lib/consent";
import { SENTRY_DSN } from "@/lib/sentry";
import { pageSocial } from "@/lib/seo";

const title = "Privacy policy: your audio stays on your machine";
// Google Analytics and the site's own Sentry error reports are described
// only in a build that has them (`NEXT_PUBLIC_GA_MEASUREMENT_ID`,
// `NEXT_PUBLIC_SENTRY_DSN`), so the page never names a service the site is
// not running. The app's crash reports are opt-in and always described.
const siteUses = SENTRY_DSN
  ? GA_MEASUREMENT_ID
    ? "anonymous Vercel analytics, Sentry error reports and, only if you accept, Google Analytics"
    : "anonymous Vercel analytics and Sentry error reports"
  : GA_MEASUREMENT_ID
    ? "anonymous Vercel analytics and, only if you accept, Google Analytics"
    : "anonymous Vercel analytics";
const description = `How edytlab handles your data: audio stays local, LLM keys and a ChatGPT sign-in token live in the OS keychain, the app sends no telemetry and sends crash reports only if you turn them on, and this site uses ${siteUses}.`;

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/privacy" },
  ...pageSocial("/privacy", `${title} · edytlab`, description),
};

export default function PrivacyPage() {
  return (
    <LegalShell title="Privacy" path="/privacy" updated="2026-11-02">
      <p>
        edytlab is a local-first desktop application. It has no server of ours
        behind it, it sends no telemetry or usage analytics, and we do not
        handle your audio. It sends crash reports only if you turn that on in
        Settings; it is off by default (see &ldquo;Crash reports&rdquo;
        below). The website is separate from the app and does use analytics,
        described under &ldquo;Telemetry and analytics&rdquo; below.
      </p>

      <h2>Audio files</h2>
      <p>
        Every audio file you load is decoded, processed, and rendered on your
        machine. Editing, mixing, and rendering run locally, and so does
        transcription, once you have downloaded the speech model; it sends no audio anywhere. Stem separation has not shipped; it
        is built to run locally too once it does. We do not upload, copy,
        mirror, or otherwise transmit your audio. There are no &ldquo;cloud
        projects&rdquo; and there is no server-side processing.
      </p>

      <h2>LLM API keys</h2>
      <p>
        edytlab supports six API providers (Anthropic, OpenAI, Google Gemini, Groq, and OpenRouter,
        plus Ollama for a local model, which needs no key) and your ChatGPT plan,
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
      <p>
        All your keys, and the ChatGPT sign-in token below, are kept together
        in one entry of that store, so macOS asks for your login password at
        most once per launch rather than once per key. What you chose (the
        active provider, each provider&apos;s model, a base URL, a reasoning
        effort) is not a secret and is not in the store: it is in a small
        settings file, <code>provider-settings.json</code>, in the
        app&apos;s config folder, and no key is ever written to it. An install
        that kept its keys one entry each moves them into the single entry the
        first time it starts and deletes the old entries only once the new
        ones read back; that first launch may ask once for each old entry.
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
        MCP server you add is contacted at the address you gave it, and the speech model and the speaker model are each downloaded from
        Hugging Face when you click their Download (below). If you turn on
        Send crash reports, which is off by default, an unexpected error also
        sends a scrubbed report to Sentry (an address under{" "}
        <code>sentry.io</code>; see &ldquo;Crash reports&rdquo; below); with
        it off, the app makes no request to Sentry. Your OS network tools show every request.
      </p>
      <p>
        <strong>The speech model download.</strong> Settings &rarr; Account has a Download button for the
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
        it. (An opt-in crash report, below, is only about an error, and only
        if you turned crash reports on.) Hugging Face handles the requests under{" "}
        <a href="https://huggingface.co/privacy">its own privacy policy</a>.
        Transcribing makes no network request of its own; the model is only
        read from your computer.
      </p>
      <p>
        <strong>The speaker model download.</strong> Settings &rarr; Account
        has a second Download button, for the speaker model that finds who
        speaks when (about 33 MB). It works the same way as the speech
        model&apos;s: the app contacts Hugging Face only when you click it, and
        not at launch, when Settings opens, or when the assistant asks. The
        click sends GET requests to <code>huggingface.co</code>, one for each
        of two files (a retry or a resume makes more), each fixed to one
        commit: the segmentation network from{" "}
        <a href="https://huggingface.co/onnx-community/pyannote-segmentation-3.0">
          onnx-community/pyannote-segmentation-3.0
        </a>{" "}
        (an export of pyannote&apos;s segmentation-3.0, MIT licence) and the
        voiceprint network from{" "}
        <a href="https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM">
          Wespeaker/wespeaker-voxceleb-resnet34-LM
        </a>{" "}
        (CC BY 4.0). What Hugging Face sees, the checks against SHA-256 hashes,
        Remove, and the absence of any telemetry are the same as for the
        speech model above, and it sees nothing about your audio. Using the
        model makes no network request of its own; it is only read from your
        computer.
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
        Settings &rarr; Account can run the assistant on
        your ChatGPT plan instead of an API key, through OpenAI&apos;s{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source">
          Sign in with ChatGPT
        </a>{" "}
        for open-source apps, which OpenAI documents as a preview. Until you click Continue with ChatGPT, the app makes none of them. Every request goes from your computer to OpenAI, and nothing
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
        credential store, in the same entry as your API keys (as{" "}
        <code>chatgpt_refresh_token</code>), and is replaced at every renewal.
        On Linux with no Secret Service running it is held in the kernel
        keyring until you restart, and on Windows a token that does not fit in
        Credential Manager (the entry is shared with your keys) is held in
        memory only; in either case Settings says so, and you sign in again
        after a restart.
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
        analytics. It sends crash reports only if you turn them on; they are
        off by default and described under &ldquo;Crash reports&rdquo; below.
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
      {SENTRY_DSN ? (
        <p>
          This website also sends error reports to Sentry when a page fails,
          with nothing about you attached; see{" "}
          <a href="#crash-reports">&ldquo;Crash reports&rdquo;</a> below.
        </p>
      ) : null}
      {GA_MEASUREMENT_ID ? <GoogleAnalyticsDisclosure /> : null}

      <h2 id="crash-reports">Crash reports</h2>
      <p>
        <strong>The desktop app.</strong> From the release after v0.9.3,
        Settings &rarr; Account has a <em>Send crash reports</em> switch. It is
        off by default, and while it is off the app makes no request to
        Sentry. When you turn it on and the app crashes or hits an unexpected
        error, it sends a report to{" "}
        <a href="https://sentry.io">Sentry</a> (an address under{" "}
        <code>sentry.io</code>): the error&apos;s type and message, where in
        edytlab&apos;s code it happened (the function names and source
        positions of the stack), the app version, and your operating system
        and processor. Before a report leaves your computer the app takes
        out file and project paths (your home folder and every project folder
        become placeholders, and any other path in a message is removed),
        anything in quotes (which is how your chat, the assistant&apos;s tool
        calls and their results end up in an error message), API keys and
        tokens, email addresses and your computer&apos;s name. It sends no
        record of what you did before the error, no local variables and no
        source lines, and a report never contains audio. Sentry sees the IP
        address a report comes from, as with any request. Turning the switch
        off stops reports at once. The choice is saved in{" "}
        <code>~/.edytlab/settings.json</code>, not in the credential store. A
        build made from source has no reporting address and cannot send a
        report at all. Sentry handles reports under{" "}
        <a href="https://sentry.io/privacy/">its own privacy policy</a>.
      </p>
      {SENTRY_DSN ? (
        <p>
          <strong>This website.</strong> When a page of this site hits an
          error in your browser, or the server fails to build a page, the
          site sends a report to Sentry: the error and its stack, the
          page&apos;s address without its query string, and your browser and
          operating system. The site sets no cookie for it and attaches no
          IP address, cookie, request header or form content to a report; it
          records no session replay, no performance traces and no record of
          what you did before the error. Sentry sees the IP address a report
          comes from, as with any request.
        </p>
      ) : null}

      <h2>Updates</h2>
      <p>
        The app does not check for updates and makes no request to find out
        whether a new release exists. New versions are published on the
        GitHub releases page and announced on this website; you download and
        install them yourself. It does not check for newer models either: the
        speech and speaker models&apos; files are pinned to one fixed version,
        and only your click on Download fetches them.
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
