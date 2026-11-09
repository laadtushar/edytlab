// Shared by scripts/render.ts and scripts/stills.ts: the browser, the bundle, the options.
import {spawnSync} from 'node:child_process';
import {existsSync} from 'node:fs';
import {dirname, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {projectPath} from './paths.ts';

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const WORK = projectPath(process.env.EDYTLAB_PROMO_WORK ?? resolve(ROOT, 'work'));
export const BUNDLE = projectPath(resolve(WORK, 'bundle'));
export const PUBLIC_DIR = projectPath(resolve(ROOT, '../footage'));

const FALLBACK = '/opt/pw-browsers/chromium_headless_shell-1194/chrome-linux/headless_shell';
export const BROWSER = process.env.REMOTION_BROWSER_EXECUTABLE ?? (existsSync(FALLBACK) ? FALLBACK : undefined);

/** Bundles the compositions once (the CLI does it; this folder is what renderMedia/renderStill serve). */
export function bundle(): string {
  const args = ['remotion', 'bundle', 'src/index.ts', `--out-dir=${BUNDLE}`, `--public-dir=${PUBLIC_DIR}`];
  const r = spawnSync('npx', args, {cwd: ROOT, stdio: 'inherit', env: process.env});
  if (r.status !== 0) throw new Error('remotion bundle failed');
  return BUNDLE;
}
