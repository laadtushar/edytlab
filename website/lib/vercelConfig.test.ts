/**
 * Which branches Vercel deploys (`vercel.json` → `git.deploymentEnabled`).
 *
 * The Hobby plan allows 100 deployments a day, and every deployment Vercel
 * creates counts, including one the `ignoreCommand` cancels a second later
 * ("Canceled builds are counted as full deployments",
 * https://vercel.com/docs/project-configuration/project-settings#ignored-build-step).
 * With every pushed branch deploying, a busy day of app-only pull requests
 * used the quota up and a real website change could not reach production.
 *
 * So only `main` (production, where the `ignoreCommand` still skips commits
 * that leave `website/` alone) and branches with `website` in their name
 * (previews) create a deployment at all. Vercel matches branch names with
 * minimatch, and a branch deploys if any matching rule is `true`
 * (https://vercel.com/docs/project-configuration/git-configuration), which
 * this test reproduces.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { minimatch } from "minimatch";
import { describe, expect, it } from "vitest";

type Rules = Record<string, boolean>;

const config = JSON.parse(
  readFileSync(fileURLToPath(new URL("../vercel.json", import.meta.url)), "utf8"),
) as { git?: { deploymentEnabled?: Rules | boolean }; ignoreCommand?: string };

/** Vercel's rule: any matching `true` deploys; no match means the default, `true`. */
function deploys(branch: string): boolean {
  const rules = config.git?.deploymentEnabled;
  if (typeof rules === "boolean") return rules;
  if (!rules) return true;
  const matching = Object.entries(rules).filter(([pattern]) => minimatch(branch, pattern));
  return matching.length === 0 || matching.some(([, on]) => on);
}

describe("vercel.json decides which branches deploy", () => {
  it("production deploys from main", () => {
    expect(deploys("main")).toBe(true);
  });

  it.each([
    "claude/fix/website-font",
    "claude/feature/website-changelog-v0.6",
    "claude/docs/website",
    "website-hotfix",
  ])("a branch named for the website gets a preview: %s", (branch) => {
    expect(deploys(branch)).toBe(true);
  });

  it.each([
    "claude/fix/live-store-registry",
    "claude/feature/whisper-log-mel",
    "claude/chore/release-0.6.0",
    "dependabot/cargo/serde-1.0.230",
    "some-branch",
  ])("any other branch creates no deployment: %s", (branch) => {
    expect(deploys(branch)).toBe(false);
  });

  it("main still skips the build when a commit leaves website/ alone", () => {
    expect(config.ignoreCommand).toContain("-- .");
  });
});
