// The choreography of each clip: where the camera looks, and what is circled and when, in SOURCE time (seconds into the
// clip), so any stretch of a clip can be cut with the same direction. Boxes are in CSS px of the 1600x1000 app window.
// A cut picks stretches (`segs`) and adds titles. Every ring and label here is a claim; each has a row in ../claims.md.
import {NOTE_CLAUDE, NOTE_LIVE} from './brand.ts';
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

/** The clips with the assistant in them: real Claude, through the app's own provider. */
const CLAUDE_CLIPS = new Set<ClipId>(['s1', 's3', 's4']);

interface Choreo {
  cam: FocusKF[];
  rings: Ring[];
  /** UI ticks at moments of the clip that make no sound of their own (source seconds): a tool call resolving, a tab click. */
  ticks?: {t: number; pitch?: number}[];
}

const CHOREO: Record<ClipId, Choreo> = {
  // S1. Describe it, it happens. Typed 2.1 to 9.6 s, sent about 10.1; Claude's Fade chip is running at 12.3 and resolves
  // at 15.5, the Gain chip at 16.0, the lanes redraw at 16.3 and 16.7, the reply is complete at 18.5; Play is pressed at
  // 24.9 (the mix is rendered first) and the sound runs from 26.3 to 34.1 (all measured from the footage).
  s1: {
    cam: [
      {t: 0, box: FULL, wide: box(0, 60, 1600, 780)},
      {t: 1.8, box: COMPOSER, wide: box(900, 560, 700, 380), move: 1.4},
      {t: 11.2, box: box(1240, 100, 360, 470), wide: box(1000, 100, 600, 470), move: 1.0},
      {t: 19.1, box: LANES_TOP, wide: box(0, 100, 700, 360), move: 1.1},
      {t: 24.2, box: box(100, 60, 560, 380), wide: box(100, 50, 700, 340), move: 1.0},
    ],
    rings: [
      {t0: 15.5, t1: 19.0, rect: box(1254, 209, 62, 26), label: 'fade', side: 'right'},
      {t0: 16.0, t1: 19.0, rect: box(1254, 242, 62, 26), label: 'gain', side: 'right'},
      {t0: 19.2, t1: 24.2, rect: box(132, 118, 280, 84), label: 'fades in over 4 s', side: 'below'},
      {t0: 19.4, t1: 24.2, rect: box(8, 414, 116, 38), label: '-3.0 dB', side: 'right'},
      {t0: 24.8, t1: 26.6, rect: box(741, 60, 28, 28), label: 'hear the result', side: 'below'},
    ],
  },
  // S2. Faders that stay put (#596). The slider is taken hold of at 3.9 s; the readout steps at 5.07, 5.60, 6.13, 6.67.
  s2: {
    cam: [{t: 0, box: LANE1_CONTROLS, wide: box(0, 170, 420, 130), square: box(0, 175, 340, 80)}],
    rings: [
      {
        t0: 3.9,
        t1: 9.6,
        rect: box(8, 176, 116, 46),
        side: 'right',
        labels: [
          {t0: 3.9, t1: 5.07, text: 'gain slider'},
          {t0: 5.07, t1: 5.6, text: '+0.5 dB'},
          {t0: 5.6, t1: 6.13, text: '+1.0 dB'},
          {t0: 6.13, t1: 6.67, text: '+1.5 dB'},
          {t0: 6.67, t1: 9.6, text: '+2.0 dB, no bounce back'},
        ],
      },
    ],
  },
  // S3. The History graph keeps up (#602). The first request is sent at 12.3 s, the Rename version chip is done at 14.3,
  // the head's bubble reads "chorus" (and the reply is done) at 17.8; the second is sent at 35.4, the nodes leave the graph at 40.9, the reply is done at 43.3.
  s3: {
    cam: [
      {t: 0, box: box(1240, 100, 360, 210), wide: box(1000, 100, 600, 260), square: box(1240, 125, 360, 210)},
      {t: 16.8, box: box(540, 760, 420, 210), wide: box(500, 700, 560, 260), move: 0.8},
      {t: 38.8, box: box(380, 60, 480, 880), wide: box(300, 40, 700, 920), move: 0.8},
    ],
    rings: [
      {t0: 17.8, t1: 21.6, rect: box(672, 903, 84, 34), label: 'named at once', side: 'right'},
      {t0: 41.0, t1: 44.6, rect: box(460, 86, 112, 450), label: '20 nodes became 8', side: 'right'},
    ],
  },
  // S4. Big sessions (#618). Looked down the lanes 1.7 to 5.1; typed 6.1 to 11.8 and sent; list_session at 14.1; twelve
  // Gain chips 16.1 to 20.3; the reply is done at 22.5; scrolled to the strings 26.9 to 29.5.
  s4: {
    cam: [
      {t: 1.0, box: LEFT_PANE, wide: box(0, 100, 1240, 600)},
      {t: 5.4, box: COMPOSER, wide: box(900, 560, 700, 380), move: 1.5},
      {t: 12.4, box: box(1240, 100, 360, 420), wide: box(1000, 100, 600, 420), move: 0.9},
      {t: 25.8, box: box(0, 200, 560, 400), wide: box(0, 200, 700, 380), move: 1.5},
    ],
    ticks: [{t: 14.2, pitch: 0.8}, ...Array.from({length: 12}, (_, i) => ({t: 16.2 + 0.36 * i, pitch: 0.88 + 0.045 * i}))],
    rings: [
      {t0: 14.2, t1: 16.6, rect: box(1254, 166, 112, 24), label: 'list_session', side: 'right'},
      {t0: 29.6, t1: 33.4, rect: box(6, 224, 122, 570), label: '-6.0 dB on the strings', side: 'right'},
    ],
  },
  // S5. The ONNX Runtime is bundled (groundwork only). The dialog is open from 4.3 s; the line is in view throughout.
  s5: {
    // the focus keeps the region above the dialog's model field out of frame
    cam: [{t: 0, box: box(580, 786, 330, 60), wide: box(560, 770, 480, 100), square: box(580, 780, 330, 80)}],
    // no label: the app's own line says "(bundled)" and a label over it covers the text
    rings: [{t0: 4.6, t1: 12.0, rect: box(576, 798, 330, 44)}],
  },
  // S6. Skills update (#574). The Skills tab's content (and the note) is on screen from 7.2 s, a skill is opened at 13.0.
  s6: {
    cam: [
      {t: 0, box: box(560, 207, 480, 586)},
      {t: 7.9, box: box(760, 440, 270, 190), wide: box(700, 420, 400, 200), move: 1.0},
    ],
    rings: [{t0: 8.0, t1: 12.8, rect: box(784, 528, 222, 52), label: 'unless you edited them', side: 'above'}],
  },
  // S7. The mix stays playable (#607). Play is pressed at 2.8 s and plays from 4.7; the Graph tab replaces the timeline
  // (and its player) at 9.3, so the sound stops; back on the timeline at 11.7; the second Play runs from 13.55 to 18.85.
  s7: {
    cam: [{t: 0, box: box(190, 0, 640, 420), wide: box(190, 0, 700, 420)}],
    ticks: [{t: 9.3}, {t: 11.7}],
    rings: [
      {t0: 11.7, t1: 12.9, rect: box(193, 10, 74, 32), label: 'back to the timeline', side: 'below'},
      {t0: 13.9, t1: 16.0, rect: box(741, 60, 28, 28), label: 'plays, no error', side: 'below'},
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
    note: CLAUDE_CLIPS.has(clip) ? NOTE_CLAUDE : NOTE_LIVE,
  };
}
