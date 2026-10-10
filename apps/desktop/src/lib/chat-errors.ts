/**
 * What the chat's error banner says when a send fails, and whether it
 * offers the way to Settings.
 *
 * A failed send reaches the frontend as the backend's error text, so
 * this reads text. Two kinds of failure are fixed in Settings, and a
 * Retry alone only repeats them: no agent configured, which means no key
 * at all (#250), and a key the provider refused (#414).
 */

/** "no agent configured; call set_api_key first" (`CommandError::NoAgent`). */
const NO_AGENT = /\bno agent\b|\bset_api_key\b/i;

/**
 * Where a message states the HTTP status the provider answered with.
 * The first is `ai::Error::Api`, which a chat request that gets a non-2xx
 * reply fails with: "the model provider returned an error (401): …"; the
 * others are the usual ways a status is written elsewhere. (A reply that
 * says the request did not fit the model's context is the exception: it
 * fails with `ai::Error::ContextTooSmall`, whose text states no status and
 * is not a key problem, #395.)
 *
 * A bare number is never read as a status: in "duration 403 s is longer
 * than the track (401.2 s)" both are lengths.
 */
const STATUS_PATTERNS: readonly RegExp[] = [
  /\breturned an error \((\d{3})\)/i,
  /\b(?:http|status(?: code)?)[\s:]+(\d{3})\b/i,
  /\b(\d{3}) (?:unauthorized|forbidden)\b/i,
];

/**
 * How providers word a refused key, for when the status alone does not
 * say so: "invalid x-api-key" (Anthropic), "Incorrect API key provided"
 * and `invalid_api_key` (OpenAI and the providers that copy its API),
 * "API key not valid" / `API_KEY_INVALID` (Gemini, which answers 400),
 * and `authentication_error` (Anthropic's error type, which also arrives
 * mid-stream with no status at all).
 *
 * The key's name is matched with a space, `_`, `-` or nothing between
 * the words, after anything but a letter — so "x-api-key" and
 * "invalid_api_key" count.
 */
const CREDENTIAL_WORDING = /(?:^|[^a-z])(?:api[\s_-]?key|(?:un)?authenticat)/i;

function statedStatus(message: string): number | null {
  for (const pattern of STATUS_PATTERNS) {
    const match = pattern.exec(message);
    if (match) return Number(match[1]);
  }
  return null;
}

/**
 * A rate limit or the provider's own fault: the key was not the problem
 * and waiting is the fix, even when the text mentions the key ("Rate
 * limit reached for this API key").
 */
function isTransientStatus(status: number): boolean {
  return status === 429 || status >= 500;
}

/** No agent is configured: there is no key for the provider at all (#250). */
export function isNoAgentError(message: string): boolean {
  return NO_AGENT.test(message);
}

/**
 * Whether the provider refused the key the request was sent with: a 401
 * or 403 from any provider, or a refusal in the provider's words where
 * it gives another status or none (#414).
 */
export function isCredentialError(message: string): boolean {
  // No key was sent, so none was refused — the `set_api_key` in that
  // message is the name of a command.
  if (isNoAgentError(message)) return false;

  const status = statedStatus(message);
  if (status === 401 || status === 403) return true;
  if (status !== null && isTransientStatus(status)) return false;
  return CREDENTIAL_WORDING.test(message);
}

/** Whether the person fixes this failure in Settings rather than by retrying. */
export function needsSettings(message: string): boolean {
  return isNoAgentError(message) || isCredentialError(message);
}

/**
 * The banner text for a send that failed with `err`.
 *
 * A refused key leads with what happened in words, because the
 * provider's reply is raw JSON ("authentication_error", "invalid
 * x-api-key") that does not say what to do. The reply follows in full:
 * its status and message say which key, and are what anyone reporting
 * the problem needs.
 */
export function describeChatError(err: unknown): string {
  const raw = String(
    err instanceof Error ? err.message : (err ?? "unknown error"),
  );
  if (isCredentialError(raw)) {
    return `The provider rejected the API key. Check it in Settings. Details: ${raw}`;
  }
  return `Could not complete request: ${raw}`;
}
