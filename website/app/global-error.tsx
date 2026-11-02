"use client";

/**
 * What a visitor sees when the root layout itself fails, and the report of
 * it to Sentry when the build has a DSN (#958). Next's own page otherwise;
 * see `lib/sentry.ts`.
 */

import NextError from "next/error";
import { useEffect } from "react";

import { SENTRY_DSN } from "@/lib/sentry";

export default function GlobalError({ error }: { error: Error & { digest?: string } }) {
  useEffect(() => {
    if (!SENTRY_DSN) return;
    void import("@sentry/nextjs").then((Sentry) => Sentry.captureException(error));
  }, [error]);

  return (
    <html lang="en">
      <body>
        {/* The App Router gives no status code here, so 0: Next's generic
            error message. */}
        <NextError statusCode={0} />
      </body>
    </html>
  );
}
