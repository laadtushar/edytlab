/**
 * The words the ChatGPT plan's cards say (#474 PR 6): Settings' card, and the
 * cards Chat draws for a failed turn. One module, so the two places and the
 * tests say the same thing.
 *
 * Plain, short, active voice. Two sources shape them: OpenAI's Help Center
 * article "Using your ChatGPT plan in other apps and sites" (an app has a
 * limit of its own; signing in again or retrying does not restore usage;
 * signing out of an app does not disconnect it from ChatGPT) and the gap
 * list G5 to G16 in the research for #474.
 */

import type { PlanUsageCode } from "./chat-errors";

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

export const CHATGPT_LABEL = "ChatGPT plan";

/**
 * The sign-in button's label, one of OpenAI's approved wordings. The button
 * shows OpenAI's logo beside it (`ChatGptLogo`), the approved format; the
 * label itself stays text.
 */
export const CONTINUE_LABEL = "Continue with ChatGPT";

export const SIGNED_OUT_ABOUT =
  "Uses your ChatGPT plan's usage instead of an API key. edytlab opens your browser to sign in.";

/** G7: no claim about Team or Business either way. */
export const ELIGIBILITY_NOTE =
  "Available for ChatGPT Plus and Pro. May not be available on workspace plans; that depends on OpenAI and your admin.";

export const SIGNING_IN = "Finish signing in in your browser…";

/** After about this long on the signing-in card, the hint appears (G6). */
export const SIGNING_HINT_AFTER_MS = 20_000;

/**
 * G6: a browser error page never reaches edytlab, so the card would wait for
 * the five-minute timeout. The hint says what the page may have said.
 */
export const SIGNING_HINT =
  "If your browser shows an error such as 'organization hasn't enabled access' or 'This app is unavailable', this account or workspace can't use ChatGPT plan sign-in. Use a personal Plus or Pro account, or an API key.";

export const USING_PLAN = "Using ChatGPT plan";

export function signedInAs(email: string | null): string {
  return email ? `Signed in as ${email}` : "Signed in";
}

export const PLAN_USAGE_DECLINED =
  "You're signed in, but plan usage is turned off for edytlab, so it can't use your plan yet.";

export const ENABLE_PLAN_USAGE_LABEL = "Turn plan usage back on";

export const SESSION_ENDED = "Your ChatGPT session ended.";

export const UNREAD =
  "edytlab hasn't read your saved ChatGPT sign-in, so it can't tell whether you're signed in.";

export const UNAVAILABLE = "Sign in with ChatGPT isn't available in this build of edytlab.";

export const MANAGE_USAGE_LABEL = "Manage usage";

/**
 * G16, shown with every sign-out and start over: signing out of an app does
 * not disconnect it from ChatGPT.
 */
export const DISCONNECT_LINE =
  "Signing out here does not disconnect edytlab from ChatGPT. To disconnect it, open ChatGPT settings, then Security and login, then Sign in with ChatGPT.";

export const START_OVER_QUESTION =
  "Start over? edytlab signs out and forgets how it was registered with ChatGPT, so the next sign-in registers it again. The old entry may stay in ChatGPT's settings until you disconnect it there.";

export const MODEL_NOT_LISTED =
  "This model isn't in your account's list. Pick one from the list.";

export function effortNotOffered(level: string): string {
  return `Your saved level (${level}) isn't offered by this model, so none is sent.`;
}

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

/** G5, from OpenAI's help article. */
export const USAGE_LIMIT_TITLE =
  "Your ChatGPT plan's usage limit for apps like edytlab has been reached.";
export const USAGE_LIMIT_APP_LIMIT =
  "An app can reach its own limit while your plan still has usage; you can change it in ChatGPT under Settings → Usage.";
export const USAGE_LIMIT_NO_RETRY = "Signing in again or retrying won't restore usage.";

export const COPY_REQUEST_ID_LABEL = "Copy";
export const COPIED_LABEL = "Copied";
export const COPY_FAILED_LABEL = "Couldn't copy";
export const COPY_FOR_ADMIN_LABEL = "Copy request for admin";

/**
 * What "Copy request for admin" puts on the clipboard. OpenAI's partner
 * guidelines ask for such an action but their wording is not public, so this
 * is edytlab's own, short and neutral: who asked, what happened and which
 * request, with where the limit is changed.
 */
export function adminRequestText(requestId: string | null): string {
  return (
    "edytlab, a desktop audio editor, reached the ChatGPT plan usage limit for apps in this workspace." +
    (requestId ? ` Request ID: ${requestId}.` : "") +
    " Please review the limit for edytlab under Settings → Usage in ChatGPT."
  );
}

export const SESSION_ENDED_TITLE = "Your ChatGPT session ended.";
export const SESSION_ENDED_BODY = `${CONTINUE_LABEL} to finish this message.`;
export const SESSION_ENDED_CARD_TEXT = `Your ChatGPT session ended. ${SESSION_ENDED_BODY}`;
export const RESUME_CARRIES_ON = "Signed in. The assistant will carry on from where it stopped.";
export const RESUME_RESENDS = "Signed in. Sending your message again.";
export const RESUME_NOTHING_KEPT = "Signed in. Send your message again.";
export const CONTINUE_CANCELLED = "Sign-in was cancelled. Your message is kept; continue when you're ready.";

/** One plain sentence for each failure that has no card of its own. */
export const PLAN_ERROR_SENTENCES: Record<
  Exclude<PlanUsageCode, "usage_limit" | "session_ended">,
  string
> = {
  not_eligible: "ChatGPT plan usage isn't available for this account or workspace.",
  plan_disabled: "Plan usage is turned off for edytlab on this account.",
  unavailable: "ChatGPT isn't answering right now.",
  unsupported_request: "ChatGPT's plan route did not accept this request.",
  refused: "OpenAI refused this request.",
};
