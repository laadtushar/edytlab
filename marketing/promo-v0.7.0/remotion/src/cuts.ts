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
    shot('hook', 's8a', [{a: 3.2, b: 5.6}], {titles: [T(0.1, 2.35, "What's new\nin v0.7.0", {hl: ['v0.7.0'], sub: 'Five changes you can see', big: true})]}),
    shot('faders', 's2', [{a: 1.6, b: 6.4}], {titles: [T(0.1, 4.7, 'Faders stay\nwhere you put them', {hl: ['stay'], sub: 'Four key taps, four steps (#596)'})]}),
    shot('graph', 's3', [{a: 4.1, b: 7.2}, {a: 15.1, b: 17.3}], {titles: [T(0.1, 5.2, 'The history graph\nkeeps up', {hl: ['keeps', 'up'], sub: 'Names and clean-ups show at once'})]}),
    shot('big-sessions', 's4', [{a: 7.3, b: 8.7}, {a: 8.7, b: 15.9, rate: 3.2}, {a: 18.9, b: 21.1}], {titles: [T(0.1, 5.5, '200 tracks?\nJust ask.', {hl: ['ask'], sub: 'list_session pages past the summary'})]}),
    shot('mix', 's7', [{a: 7.2, b: 8.9}, {a: 10.0, b: 12.9}], {audio: true, titles: [T(0.1, 4.5, 'Switch tabs.\nIt still plays.', {hl: ['plays'], sub: 'Even when the mix is gone from the cache'})]}),
    shot('skills', 's6', [{a: 3.9, b: 8.5}], {titles: [T(0.1, 4.5, 'Bundled skills\nstay current', {hl: ['current'], sub: 'Updated when a release changes them'})]}),
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
    shot('hook', 's1', [{a: 0.9, b: 2.7}], {titles: [T(0.1, 1.7, 'Describe\nthe edit.', {hl: ['edit'], big: true})]}),
    shot('type', 's1', [{a: 2.7, b: 6.4}], {titles: [T(0.1, 3.1, 'Say it in\nplain English', {hl: ['plain', 'english'], sub: 'Typed into the chat box'})]}),
    shot('tools', 's1', [{a: 6.4, b: 10.4}], {titles: [T(0.1, 3.9, 'The assistant\ncalls the tools', {hl: ['tools'], sub: 'fade, then set_track_gain'})]}),
    shot('play', 's1', [{a: 10.4, b: 12.0}, {a: 12.0, b: 18.4}], {
      audio: true,
      titles: [T(0.1, 1.5, 'The timeline\nchanges', {hl: ['changes']}), T(1.7, 7.8, 'Then hear it', {hl: ['hear'], sub: 'A synthetic demo track'})],
      chips: [chip(2.0, 7.7, 'demo audio, made in code', 'bl')],
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
    shot('hook', 's4', [{a: 0.6, b: 4.0}], {titles: [T(0.1, 1.7, '200 tracks?', {hl: ['200'], big: true}), T(1.8, 3.3, 'Ask.', {hl: ['ask'], big: true})]}),
    shot('ask', 's4', [{a: 6.5, b: 8.8}], {titles: [T(0.1, 2.2, 'Just say it', {hl: ['say'], sub: '“make all the strings quieter”'})]}),
    shot('pages', 's4', [{a: 8.8, b: 15.9}], {
      titles: [T(0.1, 2.5, 'It pages through\nthe session', {hl: ['pages'], sub: 'list_session, then set_track_gain'}), T(2.7, 7.0, 'Twelve strings,\none sentence', {hl: ['twelve'], sub: 'Each change is a node on the graph'})],
    }),
    shot('result', 's4', [{a: 17.0, b: 21.6}], {titles: [T(0.1, 4.5, 'All twelve strings\n4 dB quieter', {hl: ['quieter'], sub: 'Violins, viola, cello, contrabass'})]}),
    end('end', 4.2),
  ],
};

// ------------------------------------------------------------------------------ V1  edytlab v0.7.0 (16:9 and 1:1)
const v1Parts = () => [
  shot('open', 's8a', [{a: 3.1, b: 7.0}], {titles: [T(0.1, 3.8, 'edytlab v0.7.0', {hl: ['v0.7.0'], sub: 'Describe the edit. It happens.', big: true})]}),
  shot('describe', 's1', [{a: 2.5, b: 6.3, rate: 1.5}, {a: 6.3, b: 11.9}, {a: 12.0, b: 16.6}], {
    audio: true,
    titles: [T(0.1, 2.7, 'Describe the edit\nin plain English', {hl: ['plain', 'english']}), T(2.8, 8.3, 'The assistant calls the tools,\nthe timeline updates', {hl: ['tools']}), T(8.4, 13.0, 'Press play and hear it', {hl: ['hear']})],
    chips: [chip(8.6, 12.9, 'demo audio, made in code', 'bl')],
  }),
  shot('faders', 's2', [{a: 1.6, b: 6.4}], {titles: [T(0.1, 4.7, 'Faders that stay\nwhere you put them', {hl: ['stay']})], chips: [chip(0.3, 4.6, '#596', 'tr')]}),
  shot('graph-intro', 's8b', [{a: 4.4, b: 7.0}], {titles: [T(0.1, 2.5, 'Every edit is a node\nin the history graph', {hl: ['node'], })]}),
  shot('graph', 's3', [{a: 4.1, b: 7.3}, {a: 15.0, b: 17.6}], {titles: [T(0.1, 3.2, 'Name a version:\nit shows at once', {hl: ['once']}), T(3.4, 5.7, 'Compact the history:\nnodes leave at once', {hl: ['leave']})], chips: [chip(0.3, 5.6, '#602', 'tr')]}),
  shot('big-sessions', 's4', [{a: 7.0, b: 8.9}, {a: 8.9, b: 15.9, rate: 2.3}, {a: 17.4, b: 21.2}], {
    titles: [T(0.1, 1.9, '200 tracks?\nJust ask.', {hl: ['ask']}), T(2.0, 5.0, 'list_session pages\nthrough the session', {hl: ['pages']}), T(5.1, 8.6, 'Twelve strings,\n4 dB quieter', {hl: ['quieter']})],
    chips: [chip(0.3, 8.5, '#618', 'tr')],
  }),
  shot('runtime', 's5', [{a: 2.0, b: 6.6}], {
    titles: [T(0.1, 4.4, 'The ONNX Runtime now\nships in every installer', {hl: ['onnx'], })],
    chips: [chip(0.5, 4.3, 'groundwork only: no model yet', 'tr')],
  }),
  shot('skills', 's6', [{a: 3.9, b: 8.5}], {titles: [T(0.1, 4.5, 'Bundled skills update,\nunless you edited them', {hl: ['update']})], chips: [chip(0.3, 4.4, '#574', 'tr')]}),
  shot('mix', 's7', [{a: 6.0, b: 8.9}, {a: 10.0, b: 14.4}], {audio: true, titles: [T(0.1, 7.1, 'Switch tabs, press play:\nit still plays', {hl: ['plays']})], chips: [chip(0.3, 7.0, '#607', 'tr')]}),
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
