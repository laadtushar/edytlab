// The five cuts. Pure data: shots from shots.ts with the words on them. scripts/cues.ts reads this same file for the
// sound cues and the .srt; src/Root.tsx renders it.
import {shot} from './shots.ts';
import {FPS, joined, shotDur, type ChipSpec, type Cut, type EndCard, type TitleSpec} from './timeline.ts';

const OVERLAP = 9; // frames (0.3 s)

const T = (t0: number, t1: number, text: string, o: {hl?: string[]; sub?: string; big?: boolean} = {}): TitleSpec => ({t0, t1, text, ...o});
const chip = (t0: number, t1: number, text: string, at: ChipSpec['at'], tone: ChipSpec['tone'] = 'muted'): ChipSpec => ({t0, t1, text, at, tone});
const end = (id: string, dur: number): EndCard => ({kind: 'end', id, dur});

// ------------------------------------------------------------------------------ R1  What's new in v0.7.0 (9:16)
export const R1: Cut = {
  id: 'R1',
  title: "What's new in v0.7.0",
  slug: 'whats-new-9x16',
  platform: 'Instagram Reels, TikTok, YouTube Shorts',
  layout: 'portrait',
  overlap: OVERLAP,
  parts: [
    shot('hook', 's8a', [{a: 9.0, b: 11.4}], {titles: [T(0.1, 2.1, "What's new\nin v0.7.0", {hl: ['v0.7.0'], sub: 'Five changes you can see', big: true})]}),
    shot('faders', 's2', [{a: 3.4, b: 8.2}], {titles: [T(0.1, 4.5, 'Faders stay\nwhere you put them', {hl: ['stay'], sub: 'Four key taps, four steps'})]}),
    shot('graph', 's3', [{a: 12.0, b: 20.0, rate: 2}, {a: 38.8, b: 44.0, rate: 1.6}], {titles: [T(0.1, 6.9, 'The history graph\nkeeps up', {hl: ['keeps', 'up'], sub: 'Names and clean-ups show at once'})]}),
    shot('big-sessions', 's4', [{a: 9.6, b: 11.9, rate: 1.5}, {a: 11.9, b: 14.1, rate: 2.2}, {a: 14.1, b: 22.6, rate: 2}, {a: 27.0, b: 30.0}], {titles: [T(0.1, 9.4, '200 tracks?\nJust ask.', {hl: ['ask'], sub: 'list_session pages past the summary'})]}),
    shot('mix', 's7', [{a: 7.6, b: 10.4}, {a: 11.2, b: 16.0}], {audio: true, titles: [T(0.1, 7.4, 'Switch tabs.\nIt still plays.', {hl: ['plays'], sub: 'Even when the mix is gone from the cache'})]}),
    shot('skills', 's6', [{a: 7.3, b: 11.9}], {titles: [T(0.1, 4.3, 'Bundled skills\nstay current', {hl: ['current'], sub: 'Updated when a release changes them'})]}),
    end('end', 4.8),
  ],
};

// ------------------------------------------------------------------------------ R2  Describe the edit (9:16)
export const R2: Cut = {
  id: 'R2',
  title: 'Describe the edit',
  slug: 'describe-the-edit-9x16',
  platform: 'Instagram Reels, TikTok, YouTube Shorts',
  layout: 'portrait',
  overlap: OVERLAP,
  parts: [
    shot('hook', 's1', [{a: 2.0, b: 3.8}], {titles: [T(0.1, 1.5, 'Describe\nthe edit.', {hl: ['edit'], big: true})]}),
    shot('type', 's1', [{a: 3.8, b: 10.2, rate: 1.8}], {titles: [T(0.1, 3.3, 'Say it in\nplain English', {hl: ['plain', 'english'], sub: 'Typed into the chat box'})]}),
    shot('tools', 's1', [{a: 10.2, b: 15.0, rate: 2.4}, {a: 15.0, b: 19.0}], {titles: [T(0.1, 5.7, 'The assistant\ncalls the tools', {hl: ['tools'], sub: 'fade, then gain'})]}),
    shot('play', 's1', [{a: 19.0, b: 20.2}, {a: 24.2, b: 34.2}], {
      audio: true,
      titles: [T(0.1, 1.1, 'The timeline\nchanges', {hl: ['changes']}), T(1.5, 11.0, 'Then hear it', {hl: ['hear'], sub: 'A synthetic demo track'})],
      chips: [chip(2.0, 10.8, 'demo audio, made in code', 'bl')],
    }),
    end('end', 3.8),
  ],
};

// ------------------------------------------------------------------------------ R3  200 tracks? Ask. (9:16)
export const R3: Cut = {
  id: 'R3',
  title: '200 tracks? Ask.',
  slug: '200-tracks-9x16',
  platform: 'Instagram Reels, TikTok, YouTube Shorts',
  layout: 'portrait',
  overlap: OVERLAP,
  parts: [
    shot('hook', 's4', [{a: 1.5, b: 5.0}], {titles: [T(0.1, 1.7, '200 tracks?', {hl: ['200'], big: true}), T(1.8, 3.2, 'Ask.', {hl: ['ask'], big: true})]}),
    shot('ask', 's4', [{a: 6.0, b: 11.9, rate: 1.7}], {titles: [T(0.1, 3.2, 'Just say it', {hl: ['say'], sub: '“make all the strings quieter”'})]}),
    shot('pages', 's4', [{a: 11.9, b: 14.1, rate: 2.2}, {a: 14.1, b: 22.6}], {
      titles: [T(0.1, 3.0, 'It reads what\nthe summary left out', {hl: ['reads'], sub: 'list_session, then gain'}), T(3.2, 9.4, 'Twelve strings,\none sentence', {hl: ['twelve'], sub: 'Each change is a node on the graph'})],
    }),
    shot('result', 's4', [{a: 26.0, b: 33.6}], {titles: [T(0.1, 7.3, 'All twelve strings\n6 dB quieter', {hl: ['quieter'], sub: 'Violins, viola, cello, contrabass'})]}),
    end('end', 4.2),
  ],
};

// ------------------------------------------------------------------------------ V1  edytlab v0.7.0 (16:9 and 1:1)
const v1Parts = () => [
  shot('open', 's8a', [{a: 8.2, b: 12.0}], {titles: [T(0.1, 3.5, 'edytlab v0.7.0', {hl: ['v0.7.0'], sub: 'Describe the edit. It happens.', big: true})]}),
  shot('describe', 's1', [{a: 3.0, b: 10.0, rate: 2.2}, {a: 10.0, b: 15.0, rate: 2.5}, {a: 15.0, b: 19.0}, {a: 24.2, b: 33.0}], {
    audio: true,
    titles: [T(0.1, 3.1, 'Describe the edit\nin plain English', {hl: ['plain', 'english']}), T(3.3, 9.0, 'The assistant calls the tools,\nthe timeline updates', {hl: ['tools']}), T(9.2, 16.0, 'Press play and hear it', {hl: ['hear']})],
    chips: [chip(9.4, 15.9, 'demo audio, made in code', 'bl')],
  }),
  shot('faders', 's2', [{a: 3.4, b: 8.4}], {titles: [T(0.1, 4.7, 'Faders that stay\nwhere you put them', {hl: ['stay']})]}),
  shot('graph-intro', 's8b', [{a: 5.8, b: 9.8}], {titles: [T(0.1, 3.7, 'Every edit is a node\nin the history graph', {hl: ['node']})]}),
  shot('graph', 's3', [{a: 12.0, b: 20.0, rate: 2}, {a: 38.8, b: 44.0, rate: 1.6}], {titles: [T(0.1, 3.9, 'Name a version:\nit shows at once', {hl: ['once']}), T(4.1, 7.2, 'Compact the history:\nnodes leave at once', {hl: ['leave']})]}),
  shot('big-sessions', 's4', [{a: 9.6, b: 11.9, rate: 1.5}, {a: 11.9, b: 14.1, rate: 2.2}, {a: 14.1, b: 22.6, rate: 1.7}, {a: 26.8, b: 30.8}], {
    titles: [T(0.1, 2.9, '200 tracks?\nJust ask.', {hl: ['ask']}), T(3.0, 8.0, 'list_session reads what\nthe summary left out', {hl: ['reads']}), T(8.1, 11.6, 'Twelve strings,\n6 dB quieter', {hl: ['quieter']})],
  }),
  shot('runtime', 's5', [{a: 4.3, b: 10.3}], {
    titles: [T(0.1, 5.7, 'The ONNX Runtime now\nships in every installer', {hl: ['onnx']})],
    chips: [chip(0.5, 5.6, 'groundwork only: no model yet', 'tr')],
  }),
  shot('skills', 's6', [{a: 7.3, b: 12.7}], {titles: [T(0.1, 5.1, 'Bundled skills update,\nunless you edited them', {hl: ['update']})]}),
  shot('mix', 's7', [{a: 6.0, b: 10.8}, {a: 11.2, b: 19.2}], {audio: true, titles: [T(0.1, 12.5, 'Switch tabs, press play:\nit still plays', {hl: ['plays']})]}),
  end('end', 5.4),
];

export const V1: Cut = {id: 'V1', title: 'edytlab v0.7.0', slug: '16x9', platform: 'YouTube, X, Reddit', layout: 'wide', overlap: OVERLAP, parts: v1Parts()};
export const V1SQ: Cut = {id: 'V1sq', title: 'edytlab v0.7.0 (1:1)', slug: '1x1', platform: 'LinkedIn', layout: 'square', overlap: OVERLAP, parts: v1Parts()};

/**
 * A title must be gone before the next shot's first title can start, or the two sit on top of each other for the length
 * of the cross-fade: a shot's last title ends 0.3 s before the shot does, unless the next shot carries straight on from
 * it (one continuous take, no cross-fade).
 */
function settle(cut: Cut): Cut {
  cut.parts.forEach((p, i) => {
    if (p.kind !== 'shot' || i === cut.parts.length - 1 || joined(cut, i + 1)) return;
    const limit = shotDur(p) - cut.overlap / FPS;
    for (const t of p.titles) t.t1 = Math.min(t.t1, limit);
  });
  return cut;
}

export const CUTS: Cut[] = [R1, R2, R3, V1, V1SQ].map(settle);
