// `node scripts/render.ts <composition...> [--no-bundle]` renders each composition to work/<id>.silent.mp4: the picture
// only, as near-lossless H.264 (CRF 12; tools/audio.py makes the final file). One bundle, one browser.
import {mkdirSync} from 'node:fs';
import {openBrowser, renderMedia, selectComposition} from '@remotion/renderer';
import {BROWSER, BUNDLE, WORK, bundle} from './common.ts';

const ids = process.argv.slice(2).filter((a) => !a.startsWith('--'));
if (ids.length === 0) throw new Error('usage: node scripts/render.ts <composition...> [--no-bundle]');
if (!process.argv.includes('--no-bundle')) bundle();
mkdirSync(WORK, {recursive: true});

const browser = await openBrowser('chrome', {browserExecutable: BROWSER});
for (const id of ids) {
  const composition = await selectComposition({serveUrl: BUNDLE, id, puppeteerInstance: browser});
  const output = `${WORK}/${id}.silent.mp4`;
  let last = -1;
  const t0 = Date.now();
  await renderMedia({
    serveUrl: BUNDLE,
    composition,
    codec: 'h264',
    outputLocation: output,
    puppeteerInstance: browser,
    imageFormat: 'png',
    pixelFormat: 'yuv420p',
    colorSpace: 'bt709',
    crf: 12,
    x264Preset: 'veryfast',
    muted: true,
    // The compositor keeps decoded 3200x2000 frames; left to itself it takes half the machine's memory and, with two renders
    // at once, was OOM-killed. 1 GB is plenty for footage read forwards.
    offthreadVideoCacheSizeInBytes: Number(process.env.REMOTION_VIDEO_CACHE_BYTES ?? 1_000_000_000),
    concurrency: Number(process.env.REMOTION_CONCURRENCY ?? 2),
    overwrite: true,
    onProgress: ({renderedFrames}) => {
      const pct = Math.floor((renderedFrames / composition.durationInFrames) * 10) * 10;
      if (pct !== last) {
        last = pct;
        console.log(`${id} ${pct}%  (${((Date.now() - t0) / 1000).toFixed(0)} s)`);
      }
    },
  });
  console.log(`${id}: ${output} in ${((Date.now() - t0) / 1000).toFixed(0)} s`);
}
await browser.close({silent: true});
