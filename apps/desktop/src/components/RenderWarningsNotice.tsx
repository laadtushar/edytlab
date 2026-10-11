/**
 * RenderWarningsNotice: the banner for effects a render leaves out (#532).
 *
 * The error banner's row in its warning tone, so it sits where a render
 * error does: above the timeline, in the pane the user is looking at. A
 * single warning is one sentence; several are a lead line with one line
 * each under it, scrolling rather than pushing the timeline off the screen
 * when there are many.
 *
 * The words come from `lib/renderWarnings.ts`. Nothing is drawn for no
 * notice, so a session with nothing left out looks as it did.
 */

import type { WarningNotice } from "../lib/renderWarnings";

import { ErrorBanner } from "./ErrorBanner";

export interface RenderWarningsNoticeProps {
  notice: WarningNotice | null;
  testId?: string;
  /** Offered for a notice about something that happened once (an export). */
  onDismiss?: () => void;
}

export function RenderWarningsNotice({
  notice,
  testId = "render-warnings",
  onDismiss,
}: RenderWarningsNoticeProps) {
  if (!notice) return null;
  return (
    <ErrorBanner
      tone="warning"
      testId={testId}
      dismissLabel="Dismiss warning"
      onDismiss={onDismiss}
      message={
        <>
          <p>{notice.lead}</p>
          {notice.items.length > 0 ? (
            <ul
              data-testid={`${testId}-list`}
              className="mt-1 max-h-24 list-disc space-y-0.5 overflow-y-auto pl-5 text-[var(--text-dim)]"
            >
              {notice.items.map((item, i) => (
                // Two entries can read alike, so the line is not a key.
                <li key={i}>{item}</li>
              ))}
            </ul>
          ) : null}
        </>
      }
    />
  );
}
