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
 * The newest release a visitor could actually download, from the list
 * endpoint: the fallback when there is no versioned release yet.
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

const API = "https://api.github.com/repos/laadtushar/edytlab";
const LIST_ENDPOINT = `${API}/releases?per_page=10`;
const LATEST_ENDPOINT = `${API}/releases/latest`;

/** What the page needs from one release, logging a missing installer. */
function toAssets(release: GitHubRelease): ReleaseAssets {
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
}

const ask = (url: string) =>
  fetch(url, {
    headers: { Accept: "application/vnd.github+json" },
    next: { revalidate: 3600 }, // ISR: refresh every hour
  });

/**
 * The release a visitor should download, for the version badge and the
 * download CTAs: the newest **versioned** release when there is one,
 * otherwise the newest downloadable dev build.
 *
 * ## Versioned first
 *
 * Every merge to main publishes a dev build as a prerelease
 * (`auto-release.yml`), so the newest release of all is nearly always a
 * dev build. Serving that put `0.4.0-dev.316` on the badge and handed
 * visitors an unsigned preview the day v0.4.0 shipped. `/releases/latest`
 * answers exactly the newest non-draft, non-prerelease release, so it is
 * asked first.
 *
 * ## Dev builds as the fallback
 *
 * Before v0.2.0 there was no versioned release and `/releases/latest`
 * 404'd for roughly 186 dev builds, while the page silently served a
 * placeholder (#241, #303). So a 404 there falls back to the list
 * endpoint, newest first, skipping drafts (whose assets 404 for
 * visitors). Both lookups log when they fail, because a silent
 * placeholder is indistinguishable from a real answer.
 */
export async function getLatestRelease(): Promise<ReleaseAssets> {
  try {
    const stable = await ask(LATEST_ENDPOINT);
    if (stable.ok) {
      const release = (await stable.json()) as GitHubRelease;
      if (release && release.tag_name && release.draft !== true) return toAssets(release);
    } else if (stable.status !== 404) {
      // 404 just means no versioned release yet; anything else is worth seeing.
      console.error(
        `[releases] GitHub returned ${stable.status} ${stable.statusText} for ${LATEST_ENDPOINT}; trying the dev builds`,
      );
    }

    const res = await ask(LIST_ENDPOINT);
    if (!res.ok) {
      console.error(
        `[releases] GitHub returned ${res.status} ${res.statusText} for ${LIST_ENDPOINT}; serving the placeholder`,
      );
      return FALLBACK;
    }

    const release = pickLatestRelease(await res.json());
    if (!release) {
      console.error(
        `[releases] no non-draft release in the first page from ${LIST_ENDPOINT}; serving the placeholder`,
      );
      return FALLBACK;
    }
    return toAssets(release);
  } catch (e) {
    console.error(`[releases] fetching the latest release threw; serving the placeholder`, e);
    return FALLBACK;
  }
}
