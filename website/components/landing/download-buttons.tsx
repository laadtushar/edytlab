import Link from "next/link";
import type { HTMLAttributes } from "react";
import { Apple, Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Magnetic } from "@/components/motion";
import type { ReleaseAssets } from "@/lib/releases";
import { cn } from "@/lib/utils";

/** The line under every pair of download buttons. */
export const BUILD_NOTE = "Unsigned dev builds · Mac (universal) · Windows 10/11 · Linux";

/**
 * The Mac and Windows download buttons, for the latest release.
 *
 * The hero's last scene and the closing call to action both offer the
 * download. They used to be two copies of the same markup, and the
 * copies had drifted: the closing one sent both buttons to the generic
 * releases list while labelling them "Download for Mac" and "Download
 * for Windows". One component, given the release, keeps both honest.
 *
 * An absent installer links to the release page and says so, rather
 * than sending the user to a generic link dressed as a direct download
 * (#241). Extra props land on the wrapper, which is how each caller
 * hangs its own animation hook on it.
 */
export function DownloadButtons({
  release,
  className,
  ...rest
}: { release: ReleaseAssets } & HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn("flex flex-col items-center justify-center gap-3 sm:flex-row", className)}
      {...rest}
    >
      <Magnetic className="w-full sm:w-auto">
        <Button asChild size="lg" className="glow w-full">
          <Link href={release.macUrl ?? release.releaseUrl}>
            <Apple className="size-4" />
            {release.macUrl ? "Download for Mac" : "Mac builds on GitHub"}
          </Link>
        </Button>
      </Magnetic>
      <Magnetic className="w-full sm:w-auto">
        <Button asChild size="lg" variant="outline" className="w-full">
          <Link href={release.winUrl ?? release.releaseUrl}>
            <Download className="size-4" />
            {release.winUrl ? "Download for Windows" : "Windows builds on GitHub"}
          </Link>
        </Button>
      </Magnetic>
    </div>
  );
}
