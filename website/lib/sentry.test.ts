/**
 * Sentry on the website (#958): off without a DSN, errors only with one, and
 * nothing about the visitor in a report.
 *
 * "Off" is checked where it happens: the browser's `instrumentation-client.ts`
 * and the server's `instrumentation.ts` are imported with
 * `NEXT_PUBLIC_SENTRY_DSN` unset and set, against a stand-in for
 * `@sentry/nextjs`, and without a DSN neither initialises Sentry or reports
 * anything. The SDK makes a request only after `init` with a DSN, so no
 * `init` is no request.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sentry = vi.hoisted(() => ({
  init: vi.fn(),
  captureRequestError: vi.fn(),
  captureException: vi.fn(),
}));

vi.mock("@sentry/nextjs", () => sentry);
// What `register` loads on each runtime; a stand-in, so the test sees the
// import and not the SDK's start-up.
const serverConfig = vi.hoisted(() => ({ loaded: vi.fn() }));
vi.mock("../sentry.server.config", () => {
  serverConfig.loaded();
  return {};
});

const WEBSITE = join(__dirname, "..");
const read = (file: string) => readFileSync(join(WEBSITE, file), "utf8");
const DSN = "https://0123456789abcdef0123456789abcdef@reports.example.invalid/4500000000000000";

/** Let the dynamic `import()`s and their `then`s run. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  vi.resetModules();
  sentry.init.mockClear();
  sentry.captureRequestError.mockClear();
  serverConfig.loaded.mockClear();
});

afterEach(() => {
  vi.unstubAllEnvs();
});

describe("the DSN", () => {
  it("is only a real Sentry DSN", async () => {
    const { parseSentryDsn } = await import("./sentry");
    expect(parseSentryDsn(undefined)).toBeUndefined();
    expect(parseSentryDsn("")).toBeUndefined();
    expect(parseSentryDsn("   ")).toBeUndefined();
    expect(parseSentryDsn("not a dsn")).toBeUndefined();
    expect(parseSentryDsn("http://0123456789abcdef@o1.ingest.sentry.io/1")).toBeUndefined();
    expect(parseSentryDsn(` ${DSN} `)).toBe(DSN);
  });

  it("is not in the repository", () => {
    for (const file of [
      "lib/sentry.ts",
      "instrumentation.ts",
      "instrumentation-client.ts",
      "sentry.server.config.ts",
      "sentry.edge.config.ts",
      "next.config.mjs",
      "vercel.json",
    ]) {
      expect(read(file), file).not.toMatch(/ingest(?:\.[a-z]+)?\.sentry\.io/);
    }
  });
});

describe("with no DSN", () => {
  it("the browser does not load or start Sentry", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", "");
    await import("../instrumentation-client");
    await settle();
    expect(sentry.init).not.toHaveBeenCalled();
  });

  it("the server does not load or start Sentry, and reports no request error", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", "");
    vi.stubEnv("NEXT_RUNTIME", "nodejs");
    const instrumentation = await import("../instrumentation");
    await instrumentation.register();
    expect(serverConfig.loaded).not.toHaveBeenCalled();
    await instrumentation.onRequestError(
      new Error("boom"),
      { path: "/", method: "GET", headers: {} },
      { routerKind: "App Router", routePath: "/", routeType: "render", renderSource: "react-server-components", revalidateReason: undefined },
    );
    expect(sentry.captureRequestError).not.toHaveBeenCalled();
    expect(sentry.init).not.toHaveBeenCalled();
  });
});

describe("with a DSN", () => {
  it("the browser starts Sentry for errors only, with no personal data", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", DSN);
    await import("../instrumentation-client");
    await settle();
    expect(sentry.init).toHaveBeenCalledTimes(1);
    const options = sentry.init.mock.calls[0][0];
    expect(options.dsn).toBe(DSN);
    expect(options.sendDefaultPii).toBe(false);
    expect(options.tracesSampleRate).toBe(0);
    expect(options.enableLogs).toBe(false);
    expect(options.enableMetrics).toBe(false);
    expect(options.maxBreadcrumbs).toBe(0);
    expect(options.replaysSessionSampleRate).toBeUndefined();
    expect(options.dataCollection).toMatchObject({
      userInfo: false,
      cookies: false,
      httpHeaders: false,
      httpBodies: [],
      queryParams: false,
      urlQueryParams: false,
      stackFrameVariables: false,
    });
    const kept = options.integrations(
      ["InboundFilters", "BrowserSession", "BrowserTracing", "Breadcrumbs", "GlobalHandlers", "Replay"].map((name) => ({ name })),
    );
    expect(kept.map((i: { name: string }) => i.name)).toEqual(["InboundFilters", "GlobalHandlers"]);
  });

  it("the server loads its config and reports request errors", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", DSN);
    vi.stubEnv("NEXT_RUNTIME", "nodejs");
    const instrumentation = await import("../instrumentation");
    await instrumentation.register();
    expect(serverConfig.loaded).toHaveBeenCalledTimes(1);
    const args = [
      new Error("boom"),
      { path: "/", method: "GET", headers: {} },
      { routerKind: "App Router", routePath: "/", routeType: "render", renderSource: "react-server-components", revalidateReason: undefined },
    ] as const;
    await instrumentation.onRequestError(...args);
    expect(sentry.captureRequestError).toHaveBeenCalledWith(...args);
  });
});

describe("a report", () => {
  it("leaves without the visitor, cookies, headers, query strings or breadcrumbs", async () => {
    const { scrubEvent } = await import("./sentry");
    const event = scrubEvent({
      type: undefined,
      user: { ip_address: "203.0.113.9", email: "someone@example.com" },
      server_name: "host",
      breadcrumbs: [{ message: "clicked" }],
      request: {
        url: "https://edytlab.com/docs?ref=newsletter&email=someone%40example.com#top",
        headers: { cookie: "a=b", "user-agent": "x" },
        cookies: { a: "b" },
        query_string: "ref=newsletter",
        data: "form",
      },
      exception: {
        values: [
          {
            type: "TypeError",
            value: "x is undefined",
            stacktrace: {
              frames: [{ filename: "https://edytlab.com/_next/static/a.js?v=1", abs_path: "https://edytlab.com/_next/static/a.js?v=1", vars: { a: 1 } }],
            },
          },
        ],
      },
    });
    expect(event.user).toBeUndefined();
    expect(event.server_name).toBeUndefined();
    expect(event.breadcrumbs).toBeUndefined();
    expect(event.request).toEqual({ url: "https://edytlab.com/docs" });
    const frame = event.exception!.values![0].stacktrace!.frames![0];
    expect(frame.filename).toBe("https://edytlab.com/_next/static/a.js");
    expect(frame.vars).toBeUndefined();
    expect(JSON.stringify(event)).not.toMatch(/example\.com|203\.0\.113|newsletter|cookie/);
  });
});

describe("the build step", () => {
  it("sends Sentry nothing about the build, and uploads nothing without a token", () => {
    const config = read("next.config.mjs");
    expect(config).toContain('from "@sentry/nextjs/config"');
    expect(config).toMatch(/telemetry:\s*false/);
    expect(config).toMatch(/sourcemaps:\s*\{\s*disable:\s*!sentryAuthToken\s*\}/);
    expect(config).toMatch(/release:\s*\{\s*create:\s*Boolean\(sentryAuthToken\)\s*\}/);
    expect(config).toContain("process.env.SENTRY_AUTH_TOKEN");
  });
});

describe("the privacy page", () => {
  const page = read("app/privacy/page.tsx");

  // The app's own switch is described with the release that ships it
  // (the site describes the released app); until then the page keeps the
  // section the site's paragraph links to.
  it("keeps the section the site's Sentry paragraph links to", () => {
    expect(page).toContain('id="crash-reports"');
    expect(page).toContain('href="#crash-reports"');
  });

  it("names the site's own Sentry reports only in a build that has a DSN", () => {
    expect(page).toMatch(/\{SENTRY_DSN \? \(\s*<p>\s*<strong>This website\.<\/strong>/);
  });

  it("is matched by the local-first table", () => {
    const table = read("app/use-cases/local-ai-audio-editor/page.tsx");
    expect(table).toMatch(/\{SENTRY_DSN \? " An error on a page of this site is reported to Sentry/);
  });
});
