import React from 'react';
import {OffthreadVideo, Sequence, staticFile} from 'remotion';
import {C, glow} from '../brand.ts';
import {camAt, videoTransform} from '../camera.ts';
import type {Layout} from '../layout.ts';
import {CLIPS, FPS, ringSpan, segDur, srcAt, type Shot} from '../timeline.ts';
import {CalloutRing, type PlacedRing} from './CalloutRing.tsx';
import {Chip} from './Chip.tsx';
import {GuardedDiv} from './SafeCheck.tsx';

/**
 * A virtual camera over a footage clip: the clip's 3200x2000 frames in a card, cropped and zoomed by the shot's focus
 * boxes. The callout rings and the chips draw on top, in the card's own pixels. Only the footage is real app UI; the
 * rings, chips and everything else are drawn here, over it.
 */
export const FootageCam: React.FC<{layout: Layout; shot: Shot; frame: number}> = ({layout, shot, frame}) => {
  const {card} = layout;
  const t = frame / FPS;
  const cam = camAt([...shot.cam].sort((a, b) => a.t - b.t), card, srcAt(shot.segs, t), layout.kind);
  const clip = CLIPS[shot.clip];

  // The clip, one Sequence per segment (a segment is a stretch of the source at one speed).
  let at = 0;
  const videos = shot.segs.map((g, i) => {
    const from = Math.round(at * FPS);
    const dur = Math.max(1, Math.round(segDur(g) * FPS));
    at += segDur(g);
    return (
      <Sequence key={i} from={from} durationInFrames={dur + 1} layout="none">
        <OffthreadVideo
          src={staticFile(clip.file)}
          startFrom={Math.round(g.a * FPS)}
          playbackRate={g.rate ?? 1}
          muted
          style={{position: 'absolute', left: 0, top: 0, width: 3200, height: 2000, transformOrigin: '0 0', transform: videoTransform(cam, card)}}
        />
      </Sequence>
    );
  });

  // Speed badge: a sped-up stretch says so.
  const here = (() => {
    let o = 0;
    for (const g of shot.segs) {
      const d = segDur(g);
      if (t < o + d) return g;
      o += d;
    }
    return shot.segs[shot.segs.length - 1];
  })();
  const rate = here.rate ?? 1;

  const placed: PlacedRing[] = shot.rings.flatMap((ring) => {
    const span = ringSpan(shot.segs, ring);
    if (!span) return [];
    const labels = (ring.labels ?? (ring.label ? [{t0: ring.t0, t1: ring.t1, text: ring.label}] : [])).flatMap((l) => {
      const s = ringSpan(shot.segs, l);
      return s ? [{t0: s[0], t1: s[1], text: l.text}] : [];
    });
    return [{ring, t0: span[0], t1: span[1], labels}];
  });

  return (
    <div
      style={{
        position: 'absolute',
        left: card.x,
        top: card.y,
        width: card.w,
        height: card.h,
        borderRadius: layout.radius,
        overflow: 'hidden',
        background: C.card,
        boxShadow: `${glow(1)}, inset 0 0 0 1.5px ${C.border}`,
      }}
    >
      <div style={{position: 'absolute', inset: 0, overflow: 'hidden', borderRadius: layout.radius}}>{videos}</div>
      {placed.map((p, i) => (
        <CalloutRing key={i} layout={layout} cam={cam} frame={frame} placed={p} />
      ))}
      {rate >= 1.45 ? (
        <GuardedDiv layout={layout} what="speed badge" style={{position: 'absolute', right: 18, top: 18}}>
          <Chip text={`${Number.isInteger(rate) ? rate : rate.toFixed(1)}× speed`} size={Math.round(layout.chipSize * 0.8)} tone="muted" />
        </GuardedDiv>
      ) : null}
      {shot.chips.map((c, i) => {
        if (t < c.t0 || t > c.t1) return null;
        const p = Math.min(1, (t - c.t0) / 0.25);
        const q = Math.min(1, (c.t1 - t) / 0.2);
        const pos: React.CSSProperties = {position: 'absolute', opacity: Math.min(p, q), [c.at.includes('t') ? 'top' : 'bottom']: 18, [c.at.includes('l') ? 'left' : 'right']: 18};
        return (
          <GuardedDiv key={i} layout={layout} what={`chip "${c.text}"`} style={pos}>
            <Chip text={c.text} size={layout.chipSize} tone={c.tone ?? 'violet'} />
          </GuardedDiv>
        );
      })}
    </div>
  );
};

