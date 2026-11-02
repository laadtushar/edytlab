/**
 * Sentry on the website (#958): off without a DSN, errors only with one, and
 * nothing about the visitor in a report.
 *
 * "Off" is checked where it happens: the browser's `instrumentation-client.ts`
 * and the server's `instrumentation.ts` are imported with
 * `NEXT_PUBLIC_SENTRY_DSN` unset and set, against stand-ins for
 * `@sentry/react` (which the browser reaches through `lib/sentry-browser.ts`)
 * and `@sentry/node`, and without a DSN neither initialises
 * Sentry or reports anything. The SDK makes a request only after `init` with a
 * DSN, so no `init` is no request.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const browser = vi.hoisted(() => ({
  init: vi.fn(),
  captureException: vi.fn(),
}));
const server = vi.hoisted(() => {
  const scope = { setContext: vi.fn(), addEventProcessor: vi.fn() };
  return {
    scope,
    init: vi.fn(),
    captureException: vi.fn(),
    withScope: vi.fn((callback: (s: typeof scope) => void) => callback(scope)),
  };
});

vi.mock("@sentry/react", () => browser);
vi.mock("@sentry/node", () => server);

const WEBSITE = join(__dirname, "..");
const read = (file: string) => readFileSync(join(WEBSITE, file), "utf8");
const DSN = "https://0123456789abcdef0123456789abcdef@reports.example.invalid/4500000000000000";

/**
 * Let the dynamic `import()`s and their `then`s run: the browser's import of
 * `lib/sentry-browser.ts` is loaded here first, so a `then` that was going to
 * run has had the time it needs.
 */
const settle = async () => {
  await import("./sentry-browser");
  for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve, 0));
};

const REQUEST_ERROR = [
  new Error("boom"),
  { path: "/blog/a-post?email=someone%40example.com", method: "GET", headers: { cookie: "a=b" } },
  {
    routerKind: "App Router",
    routePath: "/blog/[slug]",
    routeType: "render",
    renderSource: "react-server-components",
    revalidateReason: undefined,
  },
] as const;

beforeEach(() => {
  vi.resetModules();
  for (const fn of [browser.init, browser.captureException, server.init, server.captureException, server.withScope, server.scope.setContext, server.scope.addEventProcessor]) {
    fn.mockClear();
  }
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
      "lib/sentry-browser.ts",
      "app/global-error.tsx",
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
    expect(browser.init).not.toHaveBeenCalled();
  });

  it("the server does not load or start Sentry, and reports no request error", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", "");
    vi.stubEnv("NEXT_RUNTIME", "nodejs");
    const instrumentation = await import("../instrumentation");
    await instrumentation.register();
    await instrumentation.onRequestError(...REQUEST_ERROR);
    expect(server.init).not.toHaveBeenCalled();
    expect(server.captureException).not.toHaveBeenCalled();
  });
});

describe("with a DSN", () => {
  it("the browser starts Sentry for errors only, with no personal data", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", DSN);
    await import("../instrumentation-client");
    await settle();
    expect(browser.init).toHaveBeenCalledTimes(1);
    const options = browser.init.mock.calls[0][0];
    expect(options.dsn).toBe(DSN);
    expect(options.sendDefaultPii).toBe(false);
    // No rate at all: a rate of 0 still records spans.
    expect(options).not.toHaveProperty("tracesSampleRate");
    expect(options).not.toHaveProperty("tracesSampler");
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
      ["InboundFilters", "BrowserSession", "BrowserTracing", "Breadcrumbs", "GlobalHandlers", "Replay", "Feedback"].map((name) => ({ name })),
    );
    expect(kept.map((i: { name: string }) => i.name)).toEqual(["InboundFilters", "GlobalHandlers"]);
    expect(server.init).not.toHaveBeenCalled();
  });

  it("the server starts Sentry in the Node.js runtime for errors only", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", DSN);
    vi.stubEnv("NEXT_RUNTIME", "nodejs");
    const instrumentation = await import("../instrumentation");
    await instrumentation.register();
    expect(server.init).toHaveBeenCalledTimes(1);
    const options = server.init.mock.calls[0][0];
    expect(options.dsn).toBe(DSN);
    expect(options.sendDefaultPii).toBe(false);
    expect(options).not.toHaveProperty("tracesSampleRate");
    expect(options.beforeSend).toBeTypeOf("function");
    // `@sentry/node`'s defaults in this version, by name.
    const kept = options.integrations(
      [
        "InboundFilters",
        "FunctionToString",
        "LinkedErrors",
        "RequestData",
        "Console",
        "Http",
        "NodeFetch",
        "OnUncaughtException",
        "OnUnhandledRejection",
        "ContextLines",
        "LocalVariables",
        "Context",
        "ChildProcess",
        "ProcessSession",
        "Modules",
      ].map((name) => ({ name })),
    );
    expect(kept.map((i: { name: string }) => i.name)).toEqual([
      "InboundFilters",
      "FunctionToString",
      "LinkedErrors",
      "RequestData",
      "OnUncaughtException",
      "OnUnhandledRejection",
      "ContextLines",
      "Context",
      "Modules",
    ]);
    expect(browser.init).not.toHaveBeenCalled();
  });

  it("the server reports a request error with its path and route, and nothing else about the request", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", DSN);
    vi.stubEnv("NEXT_RUNTIME", "nodejs");
    const instrumentation = await import("../instrumentation");
    const { scrubEvent } = await import("./sentry");
    await instrumentation.onRequestError(...REQUEST_ERROR);
    expect(server.captureException).toHaveBeenCalledTimes(1);
    expect(server.captureException.mock.calls[0][0]).toBe(REQUEST_ERROR[0]);
    expect(server.scope.setContext).toHaveBeenCalledWith("nextjs", {
      router_kind: "App Router",
      route_path: "/blog/[slug]",
      route_type: "render",
    });
    // The event as the processor leaves it, then as `beforeSend` sends it.
    const processor = server.scope.addEventProcessor.mock.calls[0][0];
    const event = scrubEvent(processor({ type: undefined, request: { headers: { cookie: "a=b" } } }));
    expect(event.request).toEqual({ url: "/blog/a-post" });
    const sent = JSON.stringify([event, server.scope.setContext.mock.calls, server.captureException.mock.calls[0][1]]);
    expect(sent).not.toMatch(/example\.com|cookie|email/);
  });

  it("the root error page reports through the browser SDK, and only with a DSN", () => {
    const page = read("app/global-error.tsx");
    expect(page).toMatch(/if \(!SENTRY_DSN\) return;\s*void import\("@\/lib\/sentry-browser"\)\.then\(\(Sentry\) => Sentry\.captureException\(error\)\)/);
  });

  it("the edge runtime starts nothing and reports nothing", async () => {
    vi.stubEnv("NEXT_PUBLIC_SENTRY_DSN", DSN);
    vi.stubEnv("NEXT_RUNTIME", "edge");
    const instrumentation = await import("../instrumentation");
    await instrumentation.register();
    await instrumentation.onRequestError(...REQUEST_ERROR);
    expect(server.init).not.toHaveBeenCalled();
    expect(server.captureException).not.toHaveBeenCalled();
  });
});

describe("the browser's chunk", () => {
  // A dynamic `import("@sentry/react")` loads the whole namespace, which the
  // bundler cannot trim: replay, feedback and tracing came along (471 KB
  // minified against 82 KB). The browser goes through one local module that
  // names what it uses.
  it("is the local module, which imports the SDK by name", async () => {
    const source = read("lib/sentry-browser.ts");
    const code = source.replace(/\/\*[\s\S]*?\*\//g, "").trim();
    expect(code).toBe('export { captureException, init } from "@sentry/react";');
    expect(Object.keys(await import("./sentry-browser")).sort()).toEqual(["captureException", "init"]);
  });

  it("is the only way the browser loads the SDK", () => {
    expect(read("instrumentation-client.ts")).toContain('import("./lib/sentry-browser")');
    expect(read("app/global-error.tsx")).toContain('import("@/lib/sentry-browser")');
    for (const file of ["instrumentation-client.ts", "app/global-error.tsx"]) {
      expect(read(file), file).not.toMatch(/["']@sentry\//);
    }
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

describe("the build", () => {
  it("has no Sentry step, and the SDKs are pinned to one version", () => {
    expect(read("next.config.mjs")).not.toMatch(/sentry/i);
    const { dependencies, devDependencies } = JSON.parse(read("package.json"));
    const sentry = Object.entries({ ...dependencies, ...devDependencies }).filter(([name]) => name.startsWith("@sentry/"));
    expect(sentry.map(([name]) => name).sort()).toEqual(["@sentry/node", "@sentry/react"]);
    expect(new Set(sentry.map(([, version]) => version)).size).toBe(1);
    for (const [, version] of sentry) expect(version).toMatch(/^10\.\d+\.\d+$/);
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
