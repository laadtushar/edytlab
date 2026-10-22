// The choreography of each clip: where the camera looks, and what is circled and when, in SOURCE time (seconds into the
// clip), so any stretch of a clip can be cut with the same direction. Boxes are in CSS px of the 1600x1000 app window.
// A cut picks stretches (`segs`) and adds titles. Every ring and label here is a claim; each has a row in ../claims.md.
import type {FocusKF} from './camera.ts';
import type {Box} from './layout.ts';
import type {ChipSpec, ClipId, Ring, Seg, Shot, TitleSpec} from './timeline.ts';

const box = (x: number, y: number, w: number, h: number): Box => ({x, y, w, h});

const FULL = box(0, 0, 1600, 1000);
const LEFT_PANE = box(0, 52, 1240, 920);
const CHAT_TOP = box(1240, 100, 360, 300);
const COMPOSER = box(1240, 770, 360, 170);
const LANE1_CONTROLS = box(0, 112, 340, 170);
const LANES_TOP = box(0, 100, 560, 480);

interface Choreo {
  cam: FocusKF[];
  rings: Ring[];
  /** UI ticks at moments of the clip that make no sound of their own (source seconds): a tool call resolving, a tab click. */
  ticks?: {t: number; pitch?: number}[];
}

const CHOREO: Record<ClipId, Choreo> = {
  // S1. Describe it, it happens.
  s1: {
    cam: [
      {t: 0, box: FULL, wide: box(0, 60, 1600, 780)},
      {t: 2.2, box: COMPOSER, wide: box(900, 560, 700, 380), move: 1.4},
      {t: 7.1, box: CHAT_TOP, wide: box(1000, 100, 600, 290), move: 1.0},
      {t: 10.2, box: LANES_TOP, wide: box(0, 100, 700, 360), move: 1.1},
      {t: 12.1, box: box(100, 60, 560, 380), wide: box(100, 50, 700, 340), move: 1.0},
    ],
    rings: [
      {t0: 8.3, t1: 10.6, rect: box(1254, 190, 64, 24), label: 'fade', side: 'right'},
      {t0: 9.55, t1: 10.8, rect: box(1254, 255, 112, 26), label: 'set_track_gain', side: 'right'},
      {t0: 10.4, t1: 12.2, rect: box(132, 118, 280, 84), label: 'fades in over 4 s', side: 'below'},
      {t0: 10.6, t1: 12.2, rect: box(8, 406, 116, 46), label: '-3.0 dB', side: 'right'},
      {t0: 12.3, t1: 13.6, rect: box(741, 60, 28, 28), label: 'hear the result', side: 'below'},
    ],
  },
  // S2. Faders that stay put (#596).
  s2: {
    cam: [{t: 0, box: LANE1_CONTROLS, wide: box(0, 170, 420, 130), square: box(0, 175, 340, 80)}],
    rings: [
      {
        t0: 2.0,
        t1: 6.6,
        rect: box(8, 176, 116, 46),
        side: 'right',
        labels: [
          {t0: 2.0, t1: 2.71, text: 'gain slider'},
          {t0: 2.71, t1: 3.24, text: '+0.5 dB'},
          {t0: 3.24, t1: 3.77, text: '+1.0 dB'},
          {t0: 3.77, t1: 4.31, text: '+1.5 dB'},
          {t0: 4.31, t1: 6.6, text: '+2.0 dB, no bounce back'},
        ],
      },
    ],
  },
  // S3. The History graph keeps up (#602).
  s3: {
    cam: [
      {t: 0, box: box(1240, 100, 360, 210), wide: box(1000, 100, 600, 260), square: box(1240, 125, 360, 210)},
      {t: 6.1, box: box(560, 780, 380, 170), wide: box(480, 740, 640, 230), move: 0.7},
      {t: 13.8, box: box(380, 60, 480, 880), wide: box(300, 40, 700, 920), move: 0.6},
    ],
    rings: [
      {t0: 6.33, t1: 7.5, rect: box(693, 898, 102, 38), label: 'named at once', side: 'right'},
      {t0: 16.0, t1: 17.6, rect: box(428, 82, 124, 564), label: '17 nodes became 8', side: 'right'},
    ],
  },
  // S4. Big sessions (#618).
  s4: {
    cam: [
      {t: 0.8, box: LEFT_PANE, wide: box(0, 100, 1240, 600)},
      {t: 6.9, box: COMPOSER, wide: box(900, 560, 700, 380), move: 1.5},
      {t: 8.4, box: box(1240, 100, 360, 300), wide: box(1000, 100, 600, 300), move: 0.9},
      {t: 15.2, box: box(1240, 520, 360, 330), wide: box(1000, 540, 600, 320), move: 3.8},
      {t: 17.5, box: box(0, 240, 560, 360), wide: box(0, 240, 700, 340), move: 1.5},
    ],
    // the chips resolve at about these moments (measured from the footage); the twelve make a rising run
    ticks: [{t: 9.77, pitch: 0.8}, {t: 10.53, pitch: 0.8}, ...Array.from({length: 12}, (_, i) => ({t: 10.77 + 0.34 * i, pitch: 0.88 + 0.045 * i}))],
    rings: [
      {t0: 10.1, t1: 11.6, rect: box(1254, 166, 130, 62), label: 'list_session', side: 'right'},
      {t0: 19.63, t1: 21.4, rect: box(6, 302, 122, 284), label: '-4.0 dB on the strings', side: 'right'},
    ],
  },
  // S5. The ONNX Runtime is bundled (groundwork only).
  s5: {
    // the focus keeps the region below the dialog's model field, which these shots do not need
    cam: [{t: 0, box: box(560, 700, 480, 216), wide: box(560, 690, 480, 240), square: box(570, 700, 392, 230)}],
    // no label: the app's own line says "(bundled)" and a label over it covers the text above
    rings: [{t0: 4.0, t1: 7.2, rect: box(576, 790, 310, 46)}],
  },
  // S6. Skills update (#574).
  s6: {
    cam: [
      {t: 0, box: box(560, 207, 480, 586)},
      {t: 4.7, box: box(760, 440, 270, 190), wide: box(700, 420, 400, 200), move: 1.0},
    ],
    rings: [{t0: 4.9, t1: 8.8, rect: box(782, 494, 226, 90), label: 'unless you edited them', side: 'above'}],
  },
  // S7. The mix stays playable (#607).
  s7: {
    cam: [{t: 0, box: box(190, 0, 640, 420), wide: box(190, 0, 700, 420)}],
    ticks: [{t: 6.03}, {t: 8.43}],
    rings: [
      {t0: 8.43, t1: 9.6, rect: box(193, 10, 74, 32), label: 'back to the timeline', side: 'below'},
      {t0: 10.35, t1: 12.4, rect: box(741, 60, 28, 28), label: 'plays, no error', side: 'below'},
    ],
  },
  // S8a / S8b. B-roll: nothing to read.
  s8a: {cam: [{t: 0, box: box(0, 100, 1240, 620), wide: box(0, 100, 1240, 600)}], rings: []},
  s8b: {cam: [{t: 0, box: box(0, 52, 1240, 920), wide: box(150, 52, 800, 700)}], rings: []},
};

export interface ShotOpts {
  titles?: TitleSpec[];
  chips?: ChipSpec[];
  /** The clip's own sound under the stretches played at 1x (scripts/cues.ts checks the rest is silent). */
  audio?: boolean;
  /** Replace the clip's rings (default: all of them) with a chosen few. */
  rings?: (r: Ring) => boolean;
  /** Replace the camera. */
  cam?: FocusKF[];
}

/** A shot of `clip`: the stretches in `segs`, directed by the clip's choreography. */
export function shot(id: string, clip: ClipId, segs: Seg[], opts: ShotOpts = {}): Shot {
  const c = CHOREO[clip];
  return {
    kind: 'shot',
    id,
    clip,
    segs,
    cam: opts.cam ?? c.cam,
    rings: opts.rings ? c.rings.filter(opts.rings) : c.rings,
    chips: opts.chips ?? [],
    titles: opts.titles ?? [],
    audio: opts.audio,
    ticks: c.ticks ?? [],
    note: true,
  };
}
