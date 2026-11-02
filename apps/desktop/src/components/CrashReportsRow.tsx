/**
 * "Send crash reports" in Settings → Account (#958): the switch for the
 * app's opt-in error reports, off by default.
 *
 * The choice is the backend's (`get_crash_reports` / `set_crash_reports`,
 * saved in `~/.edytlab/settings.json`), and it takes effect at once: on
 * makes the reporting client, off closes it. The row says what a report
 * holds and what is taken out of it before it leaves, and a build without a
 * reporting address (any build from source) says it cannot send and keeps
 * the switch off.
 */

import { useEffect, useState } from "react";

import { CRASH_REPORTS_COPY } from "../lib/crashReportsCopy";
import { getCrashReports, setCrashReports, type CrashReportsStatus } from "../lib/tauri-bridge";

type View =
  | { state: "loading" }
  | { state: "shown"; status: CrashReportsStatus; saving: boolean; error: string | null }
  | { state: "failed"; reason: string };

const reasonOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function CrashReportsRow() {
  const [view, setView] = useState<View>({ state: "loading" });

  useEffect(() => {
    let live = true;
    getCrashReports().then(
      (status) => live && setView({ state: "shown", status, saving: false, error: null }),
      (e: unknown) => live && setView({ state: "failed", reason: reasonOf(e) }),
    );
    return () => {
      live = false;
    };
  }, []);

  const choose = (enabled: boolean) => {
    if (view.state !== "shown") return;
    setView({ ...view, saving: true, error: null });
    setCrashReports(enabled).then(
      (status) => setView({ state: "shown", status, saving: false, error: null }),
      (e: unknown) => {
        // The choice may still apply to this launch: ask what is in force.
        const error = reasonOf(e);
        getCrashReports().then(
          (status) => setView({ state: "shown", status, saving: false, error }),
          () => setView({ ...view, saving: false, error }),
        );
      },
    );
  };

  const status = view.state === "shown" ? view.status : null;
  const available = status?.available ?? false;

  return (
    <div data-testid="settings-crash-reports" className="mt-3 border-t border-[var(--border)] pt-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <label
            htmlFor="settings-crash-reports-toggle"
            className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]"
          >
            {CRASH_REPORTS_COPY.label}
          </label>
          <p className="mt-1 text-xs text-[var(--text-dim)]">{CRASH_REPORTS_COPY.what}</p>
          <p className="mt-1 text-xs text-[var(--text-dim)]">{CRASH_REPORTS_COPY.removed}</p>
          {status && !available ? (
            <p data-testid="settings-crash-reports-unavailable" className="mt-1 text-xs text-[var(--text-faint)]">
              {CRASH_REPORTS_COPY.unavailable}
            </p>
          ) : null}
        </div>
        <input
          id="settings-crash-reports-toggle"
          data-testid="settings-crash-reports-toggle"
          type="checkbox"
          role="switch"
          className="mt-0.5 shrink-0"
          aria-checked={status ? status.enabled && available : false}
          checked={status ? status.enabled && available : false}
          disabled={!status || !available || (view.state === "shown" && view.saving)}
          onChange={(e) => choose(e.target.checked)}
        />
      </div>
      {view.state === "shown" && view.error ? (
        <p data-testid="settings-crash-reports-error" role="alert" className="mt-2 text-xs text-[var(--warning)]">
          {view.error}
        </p>
      ) : null}
      {view.state === "failed" ? (
        <p data-testid="settings-crash-reports-error" role="alert" className="mt-2 text-xs text-[var(--warning)]">
          {view.reason}
        </p>
      ) : null}
    </div>
  );
}
