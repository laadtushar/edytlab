// QC stills: `node scripts/stills.ts <composition> <outDir> <seconds...>` renders one PNG per time, with one browser and
// one bundle (a `remotion still` per frame would start Chromium each time). `--no-bundle` reuses work/bundle.
import {mkdirSync} from 'node:fs';
import {openBrowser, renderStill, selectComposition} from '@remotion/renderer';
import {BROWSER, BUNDLE, bundle} from './common.ts';

const args = process.argv.slice(2).filter((a) => !a.startsWith('--'));
const [id, outDir, ...times] = args;
if (!id || !outDir || times.length === 0) throw new Error('usage: node scripts/stills.ts <composition> <outDir> <seconds...> [--no-bundle]');
if (!process.argv.includes('--no-bundle')) bundle();
mkdirSync(outDir, {recursive: true});

const browser = await openBrowser('chrome', {browserExecutable: BROWSER, chromiumOptions: {}});
const composition = await selectComposition({serveUrl: BUNDLE, id, puppeteerInstance: browser});
for (const s of times) {
  const frame = Math.min(composition.durationInFrames - 1, Math.round(parseFloat(s) * composition.fps));
  const output = `${outDir}/${id}_${parseFloat(s).toFixed(1).padStart(5, "0")}s.png`;
  await renderStill({serveUrl: BUNDLE, composition, frame, output, puppeteerInstance: browser, imageFormat: 'png', offthreadVideoCacheSizeInBytes: 500_000_000});
  console.log(output);
}
await browser.close({silent: true});
