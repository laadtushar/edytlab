/** The "chatgpt-plan" page of the user guide: moved from the one long page, unchanged. */
export function ChatGptPlan() {
  return (
    <>
      <p>
        edytlab can run the assistant on a ChatGPT plan instead of
        an API key, using OpenAI&apos;s{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source">
          Sign in with ChatGPT
        </a>{" "}
        for open-source apps, which OpenAI documents as a{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations">
          preview
        </a>
        . Requests count toward the usage included in your plan, with no
        per-token bill. Available for ChatGPT Plus and Pro. May not be
        available on workspace plans; that depends on OpenAI and your admin.
        edytlab is not affiliated with OpenAI, which owns the ChatGPT name.
      </p>

      <h3>Sign in</h3>
      <ol>
        <li>
          Open Settings (the gear icon) &rarr; Account and choose{" "}
          <strong>ChatGPT plan</strong>. A card replaces the key, address,
          model and Test fields.
        </li>
        <li>
          Choose <strong>Continue with ChatGPT</strong>. edytlab opens
          OpenAI&apos;s sign-in page in your browser, and the card reads
          &ldquo;Finish signing in in your browser&hellip;&rdquo; with a{" "}
          <strong>Cancel</strong> button. You have up to five minutes.
        </li>
        <li>
          Sign in, review what edytlab asks for, and approve. The card reads
          &ldquo;Signed in as &hellip;&rdquo; with a{" "}
          <strong>Using ChatGPT plan</strong> badge.
        </li>
      </ol>
      <p>
        A browser error such as &ldquo;organization hasn&apos;t enabled
        access&rdquo; or &ldquo;This app is unavailable&rdquo; is OpenAI&apos;s
        and never reaches edytlab: this account or workspace can&apos;t use
        ChatGPT plan sign-in. After about 20 seconds the card says so. Use a
        personal Plus or Pro account, or an API key.
      </p>

      <h3>The card</h3>
      <ul>
        <li>
          A <strong>model</strong> list, read from your account (the first is
          chosen if you have not picked one), and, when the model lists
          reasoning levels, a <strong>Reasoning effort</strong> list. Higher
          levels think longer and use more of your plan.
        </li>
        <li>
          <strong>Manage usage</strong>, which opens ChatGPT&apos;s Settings
          &rarr; Usage page in your browser, where you see what edytlab and
          your other apps have used and set a weekly limit for edytlab.
          edytlab has no usage meter of its own.
        </li>
        <li>
          <strong>Sign out</strong>, which asks OpenAI to end the sign-in and
          removes it from this computer.
        </li>
        <li>
          <strong>Use another account</strong>, which signs the first account
          out once the second has signed in.
        </li>
        <li>
          <strong>Start over</strong> (it asks first), which signs out and
          forgets how edytlab was registered with ChatGPT, so the next sign-in
          registers again. The old entry may stay in ChatGPT&apos;s settings
          until you disconnect it there.
        </li>
      </ul>
      <p>
        If you turned off plan usage for edytlab on OpenAI&apos;s page, the card
        says so and offers <strong>Turn plan usage back on</strong>. It also
        says when edytlab cannot read its saved sign-in, or the keychain keeps
        it only until you restart (Linux with no Secret Service, say).
      </p>
      <p>
        <strong>Signing out here does not disconnect edytlab from ChatGPT.</strong>{" "}
        The card says so each time, because OpenAI&apos;s Help Center says that
        signing out of an app &ldquo;does not disconnect it from ChatGPT or
        stop its authorized use of your plan&rdquo;. To disconnect edytlab,
        open ChatGPT settings, then Security and login, then Sign in with
        ChatGPT, select edytlab and choose Disconnect (
        <a href="https://help.openai.com/en/articles/20001542-using-your-chatgpt-plan-in-other-apps-and-sites">
          OpenAI Help Center
        </a>
        ). If edytlab could not confirm that OpenAI ended the sign-in, the
        card tells you to do this.
      </p>

      <h3>Limits and what happens when you reach them</h3>
      <ul>
        <li>
          Each step the assistant takes draws on your plan, and on Plus the
          five-hour limit is shared with every other app and tool that uses
          the plan; it does not apply to Pro (
          <a href="https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions">
            OpenAI
          </a>
          ). To spend less, edytlab skips the small extra request it makes with
          other providers to guess the kind of edit before each message. Plan
          first works as with every provider.
        </li>
        <li>
          An app can reach its own weekly limit while your plan still has
          usage. When a request is refused for usage, Chat shows a card with{" "}
          <strong>Manage usage</strong>, the request ID with a{" "}
          <strong>Copy</strong> button (to quote to OpenAI Support) and{" "}
          <strong>Copy request for admin</strong>, a short note for a workspace
          administrator. There is no Retry: signing in again or retrying does
          not restore usage, and edytlab does not retry or switch to another
          provider or an API key. Letting apps use ChatGPT credits once the
          plan&apos;s usage runs out is a separate choice you make in ChatGPT,
          and edytlab&apos;s limit there must be 100%.
        </li>
        <li>
          When the session ends (disconnected, or OpenAI would not renew it),
          Chat shows &ldquo;Your ChatGPT session ended. Continue with ChatGPT
          to finish this message.&rdquo; <strong>Continue with ChatGPT</strong>{" "}
          signs you in again and resends your message, or, if the assistant had
          started work, lets it carry on from where it stopped. Cancel keeps
          the card and the message. The message is held in memory only, so
          quitting edytlab forgets it.
        </li>
        <li>
          Other refusals (an account or workspace OpenAI does not allow, plan
          usage turned off, ChatGPT not answering, a request the plan route
          does not accept) get one sentence and the request ID. ChatGPT not
          answering offers <strong>Try again</strong>; plan usage turned off
          offers <strong>Open Settings</strong>.
        </li>
      </ul>
      <p>
        What the sign-in sends and keeps is on the{" "}
        <a href="/privacy#signing-in-with-chatgpt">privacy page</a>.
      </p>
    </>
  );
}
