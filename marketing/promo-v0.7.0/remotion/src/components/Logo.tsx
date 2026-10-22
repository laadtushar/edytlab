import React from 'react';
import {C, clamp01, easeOut} from '../brand.ts';

/**
 * The mark (website/public/logo.svg, in the viewBox of website/components/motion/logo.tsx): three level bars and two
 * rising strokes. `progress` 0..1 builds it the way the site does: the bars rise like a meter catching a signal, then
 * the strokes are drawn.
 */
export const Logo: React.FC<{size: number; progress?: number}> = ({size, progress = 1}) => {
  const bar = (i: number) => {
    const p = easeOut(clamp01((progress - i * 0.09) / 0.45));
    return 0.15 + 0.85 * p + Math.sin(p * Math.PI) * 0.06;
  };
  const line = (i: number) => 1 - easeOut(clamp01((progress - 0.35 - i * 0.12) / 0.5));
  const bars = [
    {x: 5, y: 26, h: 12},
    {x: 14, y: 20, h: 24},
    {x: 23, y: 25, h: 14},
  ];
  return (
    <svg width={size} height={size} viewBox="3.854 3.354 57.29 57.29" aria-hidden>
      <g fill={C.logoBars}>
        {bars.map((b, i) => (
          <rect key={i} x={b.x} y={b.y} width={6} height={b.h} rx={3} style={{transformBox: 'fill-box', transformOrigin: '50% 50%', transform: `scaleY(${bar(i)})`}} />
        ))}
      </g>
      <path d="M34 26 L41 18 L48 23 L57 12" fill="none" stroke={C.logoBars} strokeWidth={6} strokeLinecap="round" strokeLinejoin="round" pathLength={1} strokeDasharray={1} strokeDashoffset={line(0)} />
      <path d="M34 38 L41 46 L48 41 L57 52" fill="none" stroke={C.logoLine} strokeWidth={6} strokeLinecap="round" strokeLinejoin="round" pathLength={1} strokeDasharray={1} strokeDashoffset={line(1)} />
    </svg>
  );
};
