import React from 'react';
import {C, FONT, NOTE} from '../brand.ts';
import type {Layout} from '../layout.ts';
import {useSafe} from './SafeCheck.tsx';

/** "v0.7.0 · real app · scripted assistant": on every shot of app footage (the footage README makes it binding). */
export const HonestyNote: React.FC<{layout: Layout; opacity: number}> = ({layout, opacity}) => {
  const ref = useSafe<HTMLDivElement>(layout, 'note');
  const n = layout.note;
  const justify = layout.noteAlign === 'left' ? 'flex-start' : layout.noteAlign === 'right' ? 'flex-end' : 'center';
  return (
    <div style={{position: 'absolute', left: n.x, top: n.y, width: n.w, height: n.h, display: 'flex', alignItems: 'center', justifyContent: justify, opacity}}>
      <div ref={ref} style={{display: 'inline-flex', alignItems: 'center', gap: layout.noteSize * 0.4, fontFamily: FONT, fontWeight: 500, fontSize: layout.noteSize, letterSpacing: '0.005em', color: C.mutedForeground, whiteSpace: 'nowrap'}}>
        <span style={{width: layout.noteSize * 0.3, height: layout.noteSize * 0.3, borderRadius: 9999, background: C.primary, flex: 'none'}} />
        {NOTE}
      </div>
    </div>
  );
};
