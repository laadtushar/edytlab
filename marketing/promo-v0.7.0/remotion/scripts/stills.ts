// QC stills: `node scripts/stills.ts <composition> <outDir> <seconds...>` renders one PNG per time, with one browser and
// one bundle (a `remotion still` per frame would start Chromium each time). `--no-bundle` reuses work/bundle.
import {mkdirSync} from 'node:fs';
import {projectPath} from './paths.ts';
import {openBrowser, renderStill, selectComposition} from '@remotion/renderer';
import {BROWSER, BUNDLE, bundle} from './common.ts';

/** Render one PNG per requested time of one composition into outDir. */
async function main(): Promise<void> {
  const args = process.argv.slice(2).filter((a) => !a.startsWith('--'));
  const [id, rawOutDir, ...times] = args;
  const outDir = rawOutDir ? projectPath(rawOutDir) : undefined;
  if (!id || !outDir || times.length === 0) throw new Error('usage: node scripts/stills.ts <composition> <outDir> <seconds...> [--no-bundle]');
  // Preflight destinations before bundling or opening Chromium.
  for (const seconds of times) projectPath(`${outDir}/${id}_${Number.parseFloat(seconds).toFixed(1).padStart(5, "0")}s.png`);
  if (!process.argv.includes('--no-bundle')) bundle();
  mkdirSync(outDir, {recursive: true});

  const browser = await openBrowser('chrome', {browserExecutable: BROWSER, chromiumOptions: {}});
  const composition = await selectComposition({serveUrl: BUNDLE, id, puppeteerInstance: browser});
  for (const seconds of times) {
    const frame = Math.min(composition.durationInFrames - 1, Math.round(Number.parseFloat(seconds) * composition.fps));
    const output = projectPath(`${outDir}/${id}_${Number.parseFloat(seconds).toFixed(1).padStart(5, "0")}s.png`);
    await renderStill({serveUrl: BUNDLE, composition, frame, output, puppeteerInstance: browser, imageFormat: 'png', offthreadVideoCacheSizeInBytes: 500_000_000});
    console.log(output);
  }
  await browser.close({silent: true});
}

if (process.argv.includes('--help') || process.argv.includes('-h')) {
  console.log('usage: node scripts/stills.ts <composition> <outDir> <seconds...> [--no-bundle]\nPaths must resolve inside marketing/promo-v0.7.0. Copy external /tmp inputs into the project first.');
} else {
  await main();
}
