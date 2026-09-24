/**
 * The real app, driven over WebDriver: the checks the Chromium harness
 * (`e2e/`) cannot make, because they depend on Tauri itself — the
 * `tauri://` protocol that delivers the Content Security Policy, the
 * `asset:` protocol and its scope, and real IPC (#334).
 *
 * Not in CI yet: it needs a built app plus `tauri-driver` and the
 * platform's WebDriver. On Linux:
 *
 *   apt-get install webkit2gtk-driver xvfb
 *   cargo install tauri-driver --locked
 *   pnpm --filter @edytlab/desktop exec tauri build --debug --no-bundle
 *   Xvfb :99 & DISPLAY=:99 HOME=$(mktemp -d) tauri-driver --port 4444 &
 *   OTHER_PROJECT=<empty dir holding inside.wav> DISPLAY=:99 \
 *     node apps/desktop/e2e-native/scenario.mjs target/debug/edytlab-desktop <some.wav>
 *
 * It prints what happened as JSON. Expected: the waveform drawn, no
 * render error, fonts loaded, compare and a project outside app data at
 * status 200, `/etc/hostname` over `asset:` at 403, and exactly one
 * violation — the deliberate `connect-src https://example.com/`.
 */
const BASE = "http://127.0.0.1:4444";
async function wd(method, path, body) {
  const r = await fetch(BASE + path, { method, headers: { "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  const j = await r.json();
  if (!r.ok) throw new Error(`${method} ${path}: ${JSON.stringify(j).slice(0, 500)}`);
  return j.value;
}
export async function session(app) {
  const v = await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: app } } } });
  const id = v.sessionId;
  return {
    exec: (script, args = []) => wd("POST", `/session/${id}/execute/sync`, { script, args }),
    execAsync: (script, args = []) => wd("POST", `/session/${id}/execute/async`, { script, args }),
    close: () => wd("DELETE", `/session/${id}`),
  };
}

// Full scenario against the real app. Prints a JSON verdict.
export async function scenario(app, wav) {
  const s = await session(app);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (script, what, ms = 20000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) { if (await s.exec(script)) return true; await sleep(250); }
    throw new Error("timed out waiting for " + what);
  };
  const recorder = "window.__v = window.__v || []; if (!window.__vOn) { window.__vOn = true; document.addEventListener('securitypolicyviolation', e => window.__v.push(e.violatedDirective + ' ' + e.blockedURI)); } return true;";
  const out = {};
  try {
    await waitFor("return !!window.__TAURI_INTERNALS__ && !!document.querySelector('[data-testid=empty-state], [data-testid=ruler]')", "app");
    // Real IPC: load the file straight into the default project.
    out.load = await s.execAsync("const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke('batch_load', { paths: [arguments[0]] }).then(v => done({ ok: v }), e => done({ err: String(e) }));", [wav]);
    await s.exec("location.reload(); return true;");
    await sleep(1500);
    await waitFor("return !!document.querySelector('[data-testid=ruler]')", "ruler after reload");
    await s.exec(recorder);
    out.csp = await s.exec("return (document.querySelector('meta[http-equiv=\"Content-Security-Policy\"]') || {}).content || null");
    out.cspHeader = await s.execAsync("const done = arguments[arguments.length - 1]; fetch(location.href).then(r => done(r.headers.get('content-security-policy')), e => done('fetch failed ' + e));");
    // The lane decoded: WaveSurfer painted bars from the asset: URL.
    await waitFor(`const c = [...document.querySelectorAll('[data-testid=timeline-lane-waveform]')].flatMap(el => [...(el.firstElementChild?.shadowRoot?.querySelectorAll('canvas') || [])]);
      return c.some(cv => { const x = cv.width && cv.height && cv.getContext('2d'); if (!x) return false; const d = x.getImageData(0,0,cv.width,cv.height).data; for (let i=3;i<d.length;i+=4) if (d[i]) return true; return false; });`, "waveform ink");
    out.waveform = "drawn";
    // Preview renders through IPC and the mix player loads it over asset:.
    await s.exec("document.querySelector('[data-testid=render-preview-button]').click(); return true;");
    await waitFor("return !document.querySelector('[data-testid=render-error]') && document.body.innerText.includes('preview') || !!document.querySelector('[data-testid=status-bar-mix-stale]') === false", "preview", 30000);
    await sleep(3000);
    out.renderError = await s.exec("return document.querySelector('[data-testid=render-error]')?.innerText || null");
    // Fonts from the bundle.
    out.fonts = await s.execAsync("const done = arguments[arguments.length - 1]; document.fonts.load('400 16px \"Geist Variable\"').then(f => done(f.filter(x => x.status === 'loaded').length), e => done(String(e)));");
    // Spectrogram: the plugin starts a Worker from a blob: URL.
    out.spectrogram = await s.exec("const b = document.querySelector('[data-testid=spectrogram-btn]'); if (!b) return 'no button'; b.click(); return 'clicked';");
    await sleep(3000);
    // What the policy should refuse.
    out.remoteFetch = await s.execAsync("const done = arguments[arguments.length - 1]; fetch('https://example.com/').then(r => done('allowed ' + r.status), e => done('blocked ' + e.name));");
    out.outOfScopeAsset = await s.execAsync("const done = arguments[arguments.length - 1]; const u = window.__TAURI_INTERNALS__.convertFileSrc('/etc/hostname', 'asset'); fetch(u).then(r => done(r.status + ' ' + u), e => done('blocked ' + e.name + ' ' + u));");
    // A compare render lands in the OS temp dir; prepare_compare grants
    // exactly its two files.
    out.compare = await s.execAsync("const done = arguments[arguments.length - 1]; const I = window.__TAURI_INTERNALS__; I.invoke('get_session_head').then(h => I.invoke('prepare_compare', { a: h, b: h })).then(r => fetch(I.convertFileSrc(r.a_path, 'asset'))).then(r => done('status ' + r.status), e => done('err ' + e));");
    // A project opened outside app data: its directory is granted on open.
    out.otherProject = await s.execAsync("const done = arguments[arguments.length - 1]; const I = window.__TAURI_INTERNALS__; I.invoke('open_project', { path: arguments[0] }).then(() => fetch(I.convertFileSrc(arguments[0] + '/inside.wav', 'asset'))).then(r => done('status ' + r.status), e => done('err ' + e));", [process.env.OTHER_PROJECT]);
    await sleep(500);
    out.violations = await s.exec("return window.__v");
  } catch (e) {
    out.error = String(e);
    try { out.violations = await s.exec("return window.__v || null"); out.text = await s.exec("return document.body.innerText.slice(0, 400)"); } catch {}
  } finally {
    await s.close().catch(() => {});
  }
  console.log(JSON.stringify(out, null, 2));
}
await scenario(process.argv[2], process.argv[3]);
