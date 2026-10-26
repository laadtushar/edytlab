"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

import { resolveHash } from "@/lib/docs/reference";

/**
 * Sends an old `/docs/tools#cut_range` to the page that has it now.
 *
 * The two reference indexes used to be one long page. A link from outside
 * the site (or one typed from memory) can still carry a hash for a name
 * that lives on an area's page, and a hash is never sent to the server,
 * so only the browser can redirect it. `targets` maps an id to
 * `<area>#<id>` (`hashTargets`); a hash that is not in it stays where it is.
 *
 * It renders nothing and is an addition, not a dependency: the index lists
 * every name with a link to its page, so without scripts the page still
 * works. It also listens for `hashchange`, for a hash put on the address
 * while the page is open.
 */
export function HashRedirect({
  base,
  targets,
}: {
  /** The index page's path, `/docs/tools`. */
  base: string;
  targets: Readonly<Record<string, string>>;
}) {
  const router = useRouter();

  useEffect(() => {
    const go = () => {
      const to = resolveHash(window.location.hash, targets);
      if (to === undefined) return;
      // Replace: the index was never what the visitor meant, so Back
      // should not return to it only to be sent on again.
      router.replace(`${base}/${to}`);
    };
    go();
    window.addEventListener("hashchange", go);
    return () => window.removeEventListener("hashchange", go);
  }, [base, targets, router]);

  return null;
}
