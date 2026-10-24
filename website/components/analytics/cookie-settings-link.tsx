"use client";

import { useSyncExternalStore } from "react";

import { GA_MEASUREMENT_ID, OPEN_SETTINGS_EVENT } from "@/lib/consent";

const noSubscription = () => () => {};

/**
 * "Cookie settings", in the footer: reopens the banner so a choice can
 * be changed or withdrawn. It renders nothing when this build has no
 * measurement ID (there is then nothing to consent to), and nothing
 * before the page is interactive, so there is no button that does
 * nothing.
 */
export function CookieSettingsLink({ id = GA_MEASUREMENT_ID }: { id?: string }) {
  const mounted = useSyncExternalStore(noSubscription, () => true, () => false);
  if (!id || !mounted) return null;
  return (
    <button
      type="button"
      onClick={() => window.dispatchEvent(new Event(OPEN_SETTINGS_EVENT))}
      className="text-xs text-muted-foreground underline underline-offset-2 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      Cookie settings
    </button>
  );
}
