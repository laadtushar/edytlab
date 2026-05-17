import { siteConfig } from "./site";

export interface ReleaseAssets {
  version: string;
  macUrl: string;
  winUrl: string;
  releaseUrl: string;
}

const FALLBACK: ReleaseAssets = {
  version: siteConfig.version,
  macUrl: siteConfig.releases,
  winUrl: siteConfig.releases,
  releaseUrl: siteConfig.releases,
};

export async function getLatestRelease(): Promise<ReleaseAssets> {
  try {
    const res = await fetch(
      "https://api.github.com/repos/laadtushar/edytlab/releases/latest",
      {
        headers: { Accept: "application/vnd.github+json" },
        next: { revalidate: 3600 }, // ISR: refresh every hour
      },
    );
    if (!res.ok) return FALLBACK;

    const data = await res.json();
    const assets: { name: string; browser_download_url: string }[] =
      data.assets ?? [];

    const dmg = assets.find((a) => a.name.endsWith(".dmg"));
    const exe = assets.find((a) => a.name.endsWith("-setup.exe"));

    return {
      version: (data.tag_name as string) ?? siteConfig.version,
      macUrl: dmg?.browser_download_url ?? siteConfig.releases,
      winUrl: exe?.browser_download_url ?? siteConfig.releases,
      releaseUrl: (data.html_url as string) ?? siteConfig.releases,
    };
  } catch {
    return FALLBACK;
  }
}
