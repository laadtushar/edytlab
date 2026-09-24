/**
 * Which release the download buttons point at (#241, #303).
 *
 * `getLatestRelease` used `/releases/latest`, which answers only
 * non-draft, non-prerelease releases — and every dev build is a
 * prerelease — so for ~186 builds both buttons served the placeholder
 * and nothing could have caught it: the website had no test runner.
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

  function answer(status: number, body: unknown) {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(JSON.stringify(body), { status })),
    );
  }

  it("asks the list endpoint, never /releases/latest", async () => {
    answer(200, [published]);
    await getLatestRelease();
    const url = String(vi.mocked(fetch).mock.calls[0][0]);
    expect(url).toContain("/releases?");
    expect(url).not.toContain("/releases/latest");
  });

  it("serves the newest downloadable release", async () => {
    answer(200, [{ ...published, draft: true, tag_name: "v0.1.0-dev.413" }, published]);
    await expect(getLatestRelease()).resolves.toEqual({
      version: "v0.1.0-dev.412",
      macUrl: asset("edytlab_0.1.0_aarch64.dmg").browser_download_url,
      winUrl: asset("edytlab_0.1.0_x64-setup.exe").browser_download_url,
      releaseUrl: published.html_url,
      isFallback: false,
    });
  });

  it("serves the placeholder, and says so, when GitHub errors", async () => {
    const log = vi.spyOn(console, "error").mockImplementation(() => undefined);
    answer(404, { message: "Not Found" });
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
    answer(200, [{ ...published, assets: [asset("edytlab_0.1.0_aarch64.dmg")] }]);
    const got = await getLatestRelease();
    expect(got.isFallback).toBe(false);
    expect(got.winUrl).toBeNull();
    expect(log).toHaveBeenCalledWith(expect.stringContaining("missing installers"));
  });
});
