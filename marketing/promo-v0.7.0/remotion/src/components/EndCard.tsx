import React from 'react';
import {spring, useCurrentFrame, useVideoConfig} from 'remotion';
import {C, END, FONT, GRADIENT, clamp01} from '../brand.ts';
import type {Layout} from '../layout.ts';
import {Chip} from './Chip.tsx';
import {Logo} from './Logo.tsx';
import {GuardedDiv} from './SafeCheck.tsx';

/**
 * The end card: the mark, the edytlab wordmark, a "v0.7.0" chip, edytlab.com, and the plain facts. Nothing here is app UI.
 * Each line is measured against the safe area like every other word.
 */
export const EndCard: React.FC<{layout: Layout; opacity: number}> = ({layout, opacity}) => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const k = layout.endScale;
  const H = 820 * k;
  const pop = (delay: number, damping = 16) => spring({frame: frame - delay, fps, config: {damping, stiffness: 150, mass: 0.7}});
  const rise = (p: number): React.CSSProperties => ({opacity: clamp01(p * 1.5), transform: `translateY(${(1 - p) * 22 * k}px)`});
  const line = (text: string, size: number, p: number, style: React.CSSProperties = {}) => (
    <GuardedDiv layout={layout} what={`end card "${text}"`} style={{display: 'block', width: 'fit-content', fontFamily: FONT, fontSize: size * k, lineHeight: 1.2, whiteSpace: 'nowrap', ...rise(p), ...style}}>
      {text}
    </GuardedDiv>
  );
  const logoP = clamp01(frame / 26);
  return (
    <div style={{position: 'absolute', left: layout.safe.x, width: layout.safe.w, top: layout.endCentreY - H / 2, height: H, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', opacity}}>
      <div style={{width: 150 * k, height: 150 * k, marginBottom: 22 * k}}>
        <Logo size={150 * k} progress={logoP} />
      </div>
      {line(END.wordmark, 128, pop(8), {fontWeight: 700, letterSpacing: '-0.035em', color: C.foreground})}
      <div style={{marginTop: 20 * k, ...rise(pop(18))}}>
        <GuardedDiv layout={layout} what={`end card chip "${END.version}"`} style={{display: 'block', width: 'fit-content'}}>
          <Chip text={END.version} size={Math.round(34 * k)} tone="violet" />
        </GuardedDiv>
      </div>
      {line(END.url, 70, pop(28), {marginTop: 34 * k, fontWeight: 600, letterSpacing: '-0.015em', background: GRADIENT, WebkitBackgroundClip: 'text', backgroundClip: 'text', WebkitTextFillColor: 'transparent', paddingBottom: 6 * k})}
      {line(END.free, 40, pop(38), {marginTop: 38 * k, fontWeight: 600, color: C.foreground})}
      {line(END.platforms, 38, pop(44), {marginTop: 12 * k, fontWeight: 500, color: C.mutedForeground})}
      {line(END.unsigned, 34, pop(50), {marginTop: 12 * k, fontWeight: 500, color: C.mutedForeground})}
    </div>
  );
};
