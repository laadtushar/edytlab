import {existsSync} from 'node:fs';
import {Config} from '@remotion/cli/config';

// Footage lives one folder up, in ../footage (the committed phase 1 recordings): served as Remotion's public dir.
Config.setPublicDir('../footage');

// Rendering needs a Chromium. Remotion v4 wants its headless shell, which it downloads on first use; where that download
// is blocked, point REMOTION_BROWSER_EXECUTABLE at any chrome-headless-shell (Playwright's `chromium_headless_shell-*/
// chrome-linux/headless_shell` works). This machine's copy is the default if it is there.
const FALLBACK = '/opt/pw-browsers/chromium_headless_shell-1194/chrome-linux/headless_shell';
const browser = process.env.REMOTION_BROWSER_EXECUTABLE ?? (existsSync(FALLBACK) ? FALLBACK : undefined);
if (browser) Config.setBrowserExecutable(browser);

Config.setConcurrency(Number(process.env.REMOTION_CONCURRENCY ?? 2));
Config.setCodec('h264');
Config.setPixelFormat('yuv420p');
Config.setColorSpace('bt709');
Config.setVideoImageFormat('png');
Config.setCrf(Number(process.env.REMOTION_CRF ?? 17));
Config.setOverwriteOutput(true);
