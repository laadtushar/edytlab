import React, {useEffect, useState} from 'react';
import {cancelRender, continueRender, delayRender} from 'remotion';
import '@fontsource/inter/latin-500.css';
import '@fontsource/inter/latin-600.css';
import '@fontsource/inter/latin-700.css';
import {WEIGHTS} from '../brand.ts';

/** Holds the render until Inter (bundled by @fontsource/inter, no network) is decoded, so no frame is set in a fallback font. */
export const FontGate: React.FC<{children: React.ReactNode}> = ({children}) => {
  const [handle] = useState(() => delayRender('Inter'));
  const [ready, setReady] = useState(false);
  useEffect(() => {
    Promise.all(WEIGHTS.map((w) => document.fonts.load(`${w} 40px Inter`, 'edytlab v0.7.0 · −×0123456789')))
      .then(() => {
        const ok = WEIGHTS.every((w) => document.fonts.check(`${w} 40px Inter`));
        if (!ok) throw new Error('Inter did not load');
        setReady(true);
        continueRender(handle);
      })
      .catch((e) => cancelRender(e));
  }, [handle]);
  // Nothing is laid out (or measured) until the font is in: a fallback font would make every measurement wrong.
  return ready ? <>{children}</> : null;
};
