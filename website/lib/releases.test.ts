/**
 * Which release the download buttons point at (#241, #303).
 *
 * Before v0.2.0 there was no versioned release: `/releases/latest`
 * answered 404 for ~186 dev builds and both buttons served the
 * placeholder. Once versioned releases existed, the opposite went wrong:
 * the newest release of all is nearly always a dev build (one per merge
 * to main), so the page showed `0.4.0-dev.316` the day v0.4.0 shipped.
 * So the versioned release is asked for first, and the dev builds are
 * the fallback.
 *
 * Fixtures follow the shape of GitHub's list endpoint, newest first,
 * including the drafts that sit ahead of every published release.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

import { getLatestRelease, pickAssets, pickLatestRelease } from "./releases";
import { siteConfig } from "./site";

const asset = (name: string) => ({
  name,
  browser_download_url: `https://github.com/laadtushar/edytlab/releases/download/x/${name}`,
});

const published = {
  tag_name: "v0.1.0-dev.412",
  html_url: "https://github.com/laadtushar/edytlab/releases/tag/v0.1.0-dev.412",
  draft: false,
  prerelease: true,
  assets: [
    asset("edytlab_0.1.0_aarch64.dmg"),
    asset("edytlab_0.1.0_x64-setup.exe"),
    asset("edytlab_0.1.0_x64_en-US.msi"),
  ],
};

describe("pickLatestRelease", () => {
  it("skips drafts, whose assets 404 for visitors", () => {
    const draft = { ...published, tag_name: "v0.1.0-dev.413", draft: true };
    expect(pickLatestRelease([draft, draft, published])?.tag_name).toBe("v0.1.0-dev.412");
  });

  it("keeps prereleases, because every dev build is one", () => {
    expect(pickLatestRelease([published])?.tag_name).toBe("v0.1.0-dev.412");
  });

  it("answers null for an empty list", () => {
    expect(pickLatestRelease([])).toBeNull();
  });

  it("answers null for a body that is not a list, such as an error object", () => {
    expect(pickLatestRelease({ message: "API rate limit exceeded" })).toBeNull();
    expect(pickLatestRelease(null)).toBeNull();
  });

  it("answers null when every release is a draft", () => {
    expect(pickLatestRelease([{ ...published, draft: true }])).toBeNull();
  });
});

describe("pickAssets", () => {
  it("points Windows at the NSIS installer, not the enterprise .msi", () => {
    expect(pickAssets(published)).toEqual({
      macUrl: asset("edytlab_0.1.0_aarch64.dmg").browser_download_url,
      winUrl: asset("edytlab_0.1.0_x64-setup.exe").browser_download_url,
    });
  });

  it("falls back to the .msi when there is no NSIS installer", () => {
    const msiOnly = { assets: [asset("edytlab_0.1.0_x64_en-US.msi")] };
    expect(pickAssets(msiOnly).winUrl).toBe(asset("edytlab_0.1.0_x64_en-US.msi").browser_download_url);
  });

  it("answers null per platform when that installer is missing", () => {
    expect(pickAssets({ assets: [asset("edytlab_0.1.0_aarch64.dmg")] }).winUrl).toBeNull();
    expect(pickAssets({ assets: [asset("edytlab_0.1.0_x64-setup.exe")] }).macUrl).toBeNull();
  });

  it("answers null for both when the release has no assets at all", () => {
    expect(pickAssets({})).toEqual({ macUrl: null, winUrl: null });
  });
});

describe("getLatestRelease", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const stable = {
    tag_name: "v0.4.0",
    html_url: "https://github.com/laadtushar/edytlab/releases/tag/v0.4.0",
    draft: false,
    prerelease: false,
    assets: [asset("edytlab_0.4.0_universal.dmg"), asset("edytlab_0.4.0_x64-setup.exe")],
  };

  /** Answer `/releases/latest` and the list endpoint separately. */
  function answer(latest: [number, unknown], list: [number, unknown]) {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) => {
        const [status, body] = String(url).includes("/releases/latest") ? latest : list;
        return new Response(JSON.stringify(body), { status });
      }),
    );
  }

  it("serves the newest versioned release, not a newer dev build", async () => {
    answer([200, stable], [200, [published]]);
    await expect(getLatestRelease()).resolves.toEqual({
      version: "v0.4.0",
      macUrl: asset("edytlab_0.4.0_universal.dmg").browser_download_url,
      winUrl: asset("edytlab_0.4.0_x64-setup.exe").browser_download_url,
      releaseUrl: stable.html_url,
      isFallback: false,
    });
  });

  it("falls back to the newest downloadable dev build when there is no versioned release", async () => {
    answer([404, { message: "Not Found" }], [200, [{ ...published, draft: true, tag_name: "v0.1.0-dev.413" }, published]]);
    await expect(getLatestRelease()).resolves.toEqual({
      version: "v0.1.0-dev.412",
      macUrl: asset("edytlab_0.1.0_aarch64.dmg").browser_download_url,
      winUrl: asset("edytlab_0.1.0_x64-setup.exe").browser_download_url,
      releaseUrl: published.html_url,
      isFallback: false,
    });
  });

  it("falls back to the dev builds, and says so, when the versioned lookup errors", async () => {
    const log = vi.spyOn(console, "error").mockImplementation(() => undefined);
    answer([500, { message: "Server Error" }], [200, [published]]);
    expect((await getLatestRelease()).version).toBe("v0.1.0-dev.412");
    expect(log).toHaveBeenCalledWith(expect.stringContaining("500"));
  });

  it("serves the placeholder, and says so, when GitHub errors", async () => {
    const log = vi.spyOn(console, "error").mockImplementation(() => undefined);
    answer([404, { message: "Not Found" }], [404, { message: "Not Found" }]);
    const got = await getLatestRelease();
    expect(got.isFallback).toBe(true);
    expect(got.releaseUrl).toBe(siteConfig.releases);
    expect(log).toHaveBeenCalledWith(expect.stringContaining("404"));
  });

  it("serves the placeholder when the request throws", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.stubGlobal("fetch", vi.fn(async () => Promise.reject(new Error("offline"))));
    expect((await getLatestRelease()).isFallback).toBe(true);
  });

  it("still serves a release that lacks an installer, and logs it", async () => {
    const log = vi.spyOn(console, "error").mockImplementation(() => undefined);
    answer([200, { ...stable, assets: [asset("edytlab_0.4.0_universal.dmg")] }], [200, [published]]);
    const got = await getLatestRelease();
    expect(got.isFallback).toBe(false);
    expect(got.version).toBe("v0.4.0");
    expect(got.winUrl).toBeNull();
    expect(log).toHaveBeenCalledWith(expect.stringContaining("missing installers"));
  });
});
