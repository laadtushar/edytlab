import React from 'react';
import {useCurrentFrame} from 'remotion';
import {clamp01, easeIn, easeOut} from '../brand.ts';
import type {Layout} from '../layout.ts';
import type {Shot} from '../timeline.ts';
import {FootageCam} from './FootageCam.tsx';
import {HonestyNote} from './HonestyNote.tsx';
import {KineticTitle} from './KineticTitle.tsx';

/** One shot: the footage card (cross-fading in and out over `overlap` frames), its titles and the honesty note. */
export const ShotView: React.FC<{layout: Layout; shot: Shot; frames: number; overlapIn: number; overlapOut: number}> = ({layout, shot, frames, overlapIn, overlapOut}) => {
  const frame = useCurrentFrame();
  // The old picture goes in the first part of the overlap and the new one comes in over the last of it, so two different
  // pictures are never both half-visible for long; where two parts are one continuous shot there is no overlap and no fade.
  const enter = overlapIn === 0 ? 1 : easeOut(clamp01((frame - 0.25 * overlapIn) / (0.75 * overlapIn)));
  const leave = overlapOut === 0 ? 0 : easeIn(clamp01((frame - (frames - overlapOut)) / (0.6 * overlapOut)));
  const opacity = enter * (1 - leave);
  const lift = (1 - enter) * 26 - leave * 12;
  return (
    <>
      <div style={{position: 'absolute', left: 0, top: 0, width: layout.width, height: layout.height, opacity, transform: `translateY(${lift}px) scale(${0.985 + 0.015 * enter})`, transformOrigin: `${layout.card.x + layout.card.w / 2}px ${layout.card.y + layout.card.h / 2}px`}}>
        <FootageCam layout={layout} shot={shot} frame={frame} />
      </div>
      {layout.hook.scrim
        ? shot.titles
            .filter((t) => t.big)
            .map((t, i) => {
              const sec = frame / 30;
              const a = clamp01((sec - t.t0) / 0.3) * (1 - clamp01((sec - (t.t1 - 0.3)) / 0.3));
              return <div key={i} style={{position: 'absolute', left: layout.card.x, top: layout.card.y, width: layout.card.w, height: layout.card.h, borderRadius: layout.radius, background: 'rgba(9, 9, 11, 0.7)', opacity: a * opacity}} />;
            })
        : null}
      {shot.note ? <HonestyNote layout={layout} opacity={opacity} text={shot.note} /> : null}
      {shot.titles.map((t, i) => (
        <KineticTitle key={i} layout={layout} spec={t} frame={frame} />
      ))}
    </>
  );
};
