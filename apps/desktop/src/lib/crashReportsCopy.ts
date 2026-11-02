/**
 * The words of Settings → Account → Send crash reports (#958). Kept in one
 * place so the row and its test say the same thing, and so they can be
 * checked against the privacy page's account of what a report holds.
 */
export const CRASH_REPORTS_COPY = {
  label: "Send crash reports",
  what:
    "Off unless you turn it on. When it is on and edytlab crashes or hits an unexpected error, " +
    "it sends a report to Sentry (sentry.io): the error, where in edytlab's code it happened, " +
    "the app version, and your operating system and processor.",
  removed:
    "Before a report leaves your computer, file and project paths, quoted text (such as your " +
    "chat and the assistant's tool calls), API keys, tokens and email addresses are taken out. " +
    "It never contains audio. Turning it off stops reports at once.",
  unavailable: "This build cannot send crash reports: it was built without a reporting address.",
} as const;
