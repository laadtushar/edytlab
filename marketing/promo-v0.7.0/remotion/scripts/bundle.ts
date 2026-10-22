// `node scripts/bundle.ts` bundles the compositions into work/bundle once, so several `scripts/render.ts ... --no-bundle`
// processes can render from it in parallel.
import {bundle} from './common.ts';

bundle();
