import React from 'react';
import {spring, useCurrentFrame, useVideoConfig} from 'remotion';
import {C, FONT, GRADIENT, clamp01, easeIn} from '../brand.ts';
import type {Layout} from '../layout.ts';
import {useSafe} from './SafeCheck.tsx';
import {titleLines, type TitleSpec} from '../timeline.ts';

const EXIT = 0.22; // seconds

/**
 * The kinetic title: each word springs up into place two frames after the one before it; words in `hl` are set in the
 * site's gradient; an optional sub line follows. At `t1` the whole block lifts away. `frame` is the shot's own frame.
 */
export const KineticTitle: React.FC<{layout: Layout; spec: TitleSpec; frame: number}> = ({layout, spec, frame}) => {
  const {fps} = useVideoConfig();
  const t = frame / fps - spec.t0;
  const hook = !!spec.big;
  const size = hook ? layout.hook.size : layout.titleSize;
  const subSize = hook ? layout.hook.sub : layout.subSize;
  const box = hook ? layout.hook.box : layout.title;
  const shown = titleLines(spec, layout.subs || hook, layout.kind === 'wide' && !hook);
  const lines = shown.lines.map((l) => l.split(' ').filter(Boolean));
  const norm = (w: string) => w.replace(/[.,!?:;"“”]/g, '').toLowerCase();
  const hl = new Set((spec.hl ?? []).map(norm));
  const textRef = useSafe<HTMLDivElement>(layout, `title "${spec.text.replace(/\n/g, ' ')}"`);
  const subRef = useSafe<HTMLDivElement>(layout, `sub "${shown.sub ?? ''}"`);
  if (t < 0 || t > spec.t1 - spec.t0) return null;
  const out = clamp01((t - (spec.t1 - spec.t0 - EXIT)) / EXIT);
  let k = 0;
  const wordCount = lines.reduce((n, l) => n + l.length, 0);
  const subIn = spring({frame: frame - Math.round((spec.t0 + 0.12 * wordCount + 0.1) * fps), fps, config: {damping: 200}});
  return (
    <div style={{position: 'absolute', left: box.x, top: box.y, width: box.w, height: box.h, overflow: 'visible', display: 'flex', flexDirection: 'column', alignItems: hook && layout.hook.scrim ? 'center' : 'flex-start', justifyContent: hook && layout.hook.scrim ? 'center' : 'flex-start'}}>
      <div ref={textRef} style={{display: 'block', width: 'fit-content', maxWidth: box.w, fontFamily: FONT, fontWeight: 700, fontSize: size, lineHeight: 1.07, letterSpacing: '-0.028em', color: C.foreground}}>
        {lines.map((line, li) => (
          <div key={li} style={{whiteSpace: 'nowrap'}}>
            {line.map((w, wi) => {
              const i = k++;
              const sp = spring({frame: frame - Math.round(spec.t0 * fps) - i * 2, fps, config: {damping: 15, mass: 0.6, stiffness: 170}});
              const o = clamp01(sp * 1.6) * (1 - easeIn(out));
              const gradient = hl.has(norm(w));
              return (
                <span
                  key={wi}
                  style={{
                    display: 'inline-block',
                    marginRight: '0.26em',
                    opacity: o,
                    transform: `translateY(${(1 - sp) * size * 0.42 - easeIn(out) * size * 0.2}px)`,
                    ...(gradient ? {background: GRADIENT, WebkitBackgroundClip: 'text', backgroundClip: 'text', WebkitTextFillColor: 'transparent', paddingBottom: '0.06em'} : {}),
                  }}
                >
                  {w}
                </span>
              );
            })}
          </div>
        ))}
      </div>
      {shown.sub ? (
        <div
          ref={subRef}
          style={{
            display: 'block',
            width: 'fit-content',
            maxWidth: box.w,
            marginTop: size * 0.2,
            fontFamily: FONT,
            fontWeight: 500,
            fontSize: subSize,
            lineHeight: 1.25,
            color: C.mutedForeground,
            opacity: clamp01(subIn) * (1 - easeIn(out)),
            transform: `translateY(${(1 - subIn) * 14}px)`,
          }}
        >
          {shown.sub}
        </div>
      ) : null}
    </div>
  );
};
