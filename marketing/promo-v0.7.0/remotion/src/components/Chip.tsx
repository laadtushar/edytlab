import React from 'react';
import {C, FONT, glow, rgba} from '../brand.ts';
import type {Tone} from '../timeline.ts';

export const chipPad = (size: number) => ({x: Math.round(size * 0.62), y: Math.round(size * 0.3)});

/** A pill: the version chip, a tool name, a callout label. Presentational; the parent positions and animates it. */
export const Chip = React.forwardRef<
  HTMLDivElement,
  {text: string; size: number; tone?: Tone; style?: React.CSSProperties; dot?: boolean}
>(({text, size, tone = 'violet', style, dot}, ref) => {
  const {x, y} = chipPad(size);
  const violet = tone === 'violet';
  return (
    <div
      ref={ref}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: size * 0.32,
        padding: `${y}px ${x}px`,
        borderRadius: 9999,
        background: violet ? 'rgba(20, 12, 32, 0.92)' : rgba(C.card, 0.94),
        border: `${Math.max(2, size * 0.07)}px solid ${violet ? rgba(C.primary, 0.75) : C.border}`,
        color: violet ? C.foreground : C.mutedForeground,
        fontFamily: FONT,
        fontWeight: 600,
        fontSize: size,
        lineHeight: 1,
        letterSpacing: '-0.005em',
        whiteSpace: 'nowrap',
        boxShadow: violet ? glow(0.5) : 'none',
        ...style,
      }}
    >
      {dot ? <span style={{width: size * 0.3, height: size * 0.3, borderRadius: 9999, background: C.primary, flex: 'none'}} /> : null}
      {text}
    </div>
  );
});
Chip.displayName = 'Chip';
