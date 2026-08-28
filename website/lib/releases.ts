import { siteConfig } from "./site";

export interface ReleaseAssets {
  version: string;
  /** Direct installer URL, or `null` when the release carries no such asset. */
  macUrl: string | null;
  winUrl: string | null;
  /** The release page. Always usable, even when an asset is missing. */
  releaseUrl: string;
  /** True when this is the hardcoded placeholder rather than a real answer. */
  isFallback: boolean;
}

const FALLBACK: ReleaseAssets = {
  version: siteConfig.version,
  macUrl: null,
  winUrl: null,
  releaseUrl: siteConfig.releases,
  isFallback: true,
};

interface GitHubRelease {
  tag_name?: string;
  html_url?: string;
  draft?: boolean;
  assets?: { name: string; browser_download_url: string }[];
}

/**
 * The newest release a visitor could actually download.
 *
 * Exported for the sake of being testable in isolation — the choice of
 * *which* release is the part that was wrong, and it does not need a
 * network to check.
 *
 * Drafts are excluded because a draft is not published: its assets 404
 * for anyone without push access. Prereleases are **kept**, which is the
 * whole point — every dev build is one.
 */
export function pickLatestRelease(list: unknown): GitHubRelease | null {
  if (!Array.isArray(list)) return null;
  return (list as GitHubRelease[]).find((r) => r && r.draft !== true) ?? null;
}

/** The installer assets, or `null` per platform when none is attached. */
export function pickAssets(release: GitHubRelease): {
  macUrl: string | null;
  winUrl: string | null;
} {
  const assets = release.assets ?? [];
  const find = (pred: (name: string) => boolean) =>
    assets.find((a) => pred(a.name))?.browser_download_url ?? null;

  return {
    macUrl: find((n) => n.endsWith(".dmg")),
    // NSIS is the installer we point people at; the .msi is also built
    // but is the enterprise-deployment artifact.
    winUrl: find((n) => n.endsWith("-setup.exe")) ?? find((n) => n.endsWith(".msi")),
  };
}

/**
 * The latest downloadable release, for the version badge and the
 * download CTAs.
 *
 * ## Why not `/releases/latest`
 *
 * That endpoint returns the newest **non-draft, non-prerelease**
 * release. `release-dev.yml` publishes every dev build with
 * `prerelease: true` — deliberately, so a dev drop never steals the
 * Latest badge — so the endpoint has 404'd for this repo across roughly
 * 186 dev builds.
 *
 * Both failure arms returned the same placeholder with no logging, so
 * the page rendered `v0.1.0-dev` (a tag that does not exist) and pointed
 * both download buttons at the generic releases page, which was itself
 * the 404'ing URL. Nothing distinguished "GitHub says the version is
 * v0.1.0-dev" from "the fetch failed", which is why it went unnoticed.
 *
 * The list endpoint returns everything, newest first, so the fix is to
 * ask it and skip drafts.
 */
export async function getLatestRelease(): Promise<ReleaseAssets> {
  const endpoint =
    "https://api.github.com/repos/laadtushar/edytlab/releases?per_page=10";
  try {
    const res = await fetch(endpoint, {
      headers: { Accept: "application/vnd.github+json" },
      next: { revalidate: 3600 }, // ISR: refresh every hour
    });
    if (!res.ok) {
      // Server-side, so it lands in the deployment logs. A silent
      // fallback is indistinguishable from a real answer, and that is
      // what hid this bug.
      console.error(
        `[releases] GitHub returned ${res.status} ${res.statusText} for ${endpoint}; serving the placeholder`,
      );
      return FALLBACK;
    }

    const release = pickLatestRelease(await res.json());
    if (!release) {
      console.error(
        `[releases] no non-draft release in the first page from ${endpoint}; serving the placeholder`,
      );
      return FALLBACK;
    }

    const { macUrl, winUrl } = pickAssets(release);
    if (!macUrl || !winUrl) {
      // Not fatal — the release page still works — but it means a build
      // leg failed to upload, which is worth seeing in the logs.
      console.error(
        `[releases] ${release.tag_name} is missing installers (mac: ${macUrl ? "ok" : "none"}, windows: ${winUrl ? "ok" : "none"})`,
      );
    }

    return {
      version: release.tag_name ?? siteConfig.version,
      macUrl,
      winUrl,
      releaseUrl: release.html_url ?? siteConfig.releases,
      isFallback: false,
    };
  } catch (e) {
    console.error(`[releases] fetching ${endpoint} threw; serving the placeholder`, e);
    return FALLBACK;
  }
}
