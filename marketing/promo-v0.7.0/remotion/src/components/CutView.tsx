import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame, useVideoConfig} from 'remotion';
import {C, GRADIENT, rgba} from '../brand.ts';
import {LAYOUTS} from '../layout.ts';
import {place, type Cut, type Part} from '../timeline.ts';
import {Background} from './Background.tsx';
import {EndCard} from './EndCard.tsx';
import {FontGate} from './FontGate.tsx';
import {ShotView} from './ShotView.tsx';
import {clamp01, easeOut} from '../brand.ts';

const PartView: React.FC<{cut: Cut; part: Part; frames: number; overlapIn: number; overlapOut: number}> = ({cut, part, frames, overlapIn, overlapOut}) => {
  const layout = LAYOUTS[cut.layout];
  const frame = useCurrentFrame();
  if (part.kind === 'end') {
    const enter = overlapIn === 0 ? 1 : easeOut(clamp01((frame - 0.25 * overlapIn) / (0.75 * overlapIn)));
    return <EndCard layout={layout} opacity={enter} />;
  }
  return <ShotView layout={layout} shot={part} frames={frames} overlapIn={overlapIn} overlapOut={overlapOut} />;
};

/** A thin progress line under everything: no words, so it may sit where 9:16 keeps words out. */
const Progress: React.FC<{cut: Cut}> = ({cut}) => {
  const frame = useCurrentFrame();
  const {durationInFrames} = useVideoConfig();
  const {progress: p} = LAYOUTS[cut.layout];
  const w = p.x1 - p.x0;
  return (
    <div style={{position: 'absolute', left: p.x0, top: p.y, width: w, height: 5, borderRadius: 3, background: rgba(C.border, 0.9), overflow: 'hidden'}}>
      <div style={{width: (w * (frame + 1)) / durationInFrames, height: 5, background: GRADIENT}} />
    </div>
  );
};

/** Renders a Cut: the brand background, then every part in its own Sequence, overlapping by `cut.overlap` frames. */
export const CutView: React.FC<{cut: Cut}> = ({cut}) => {
  const layout = LAYOUTS[cut.layout];
  const {starts, frames, overlaps} = place(cut);
  const rootProps = {'data-cut-root': ''} as Record<string, string>;
  return (
    <FontGate>
      <AbsoluteFill style={{background: C.background, width: layout.width, height: layout.height}} {...rootProps}>
        <Background width={layout.width} height={layout.height} />
        <Progress cut={cut} />
        {cut.parts.map((p, i) => (
          <Sequence key={i} from={starts[i]} durationInFrames={frames[i]} layout="none" name={p.id}>
            <PartView cut={cut} part={p} frames={frames[i]} overlapIn={overlaps[i]} overlapOut={overlaps[i + 1] ?? 0} />
          </Sequence>
        ))}
      </AbsoluteFill>
    </FontGate>
  );
};
