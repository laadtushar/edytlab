import React, {useEffect, useState} from 'react';
import {AbsoluteFill, cancelRender, continueRender, delayRender, useCurrentFrame} from 'remotion';
import {C, rgba} from '../brand.ts';

/** The site's film grain (`.grain::after` in website/app/globals.css): a 120 px fractal-noise tile. */
const GRAIN =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='120' height='120'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='3'/%3E%3C/filter%3E%3Crect width='120' height='120' filter='url(%23n)'/%3E%3C/svg%3E\")";
const GRAIN_SRC = GRAIN.slice(5, -2);

/**
 * The website's `.aurora` behind a section (a violet and a magenta wash at opposite corners, opacity 0.16, drifting
 * slowly) on the page background, with the film grain on top: the grain also keeps the dark gradient from banding once it is
 * encoded. The washes are radial gradients rather than blurred discs, which looks the same and costs far less per frame.
 */
export const Background: React.FC<{width: number; height: number}> = ({width, height}) => {
  const frame = useCurrentFrame();
  const [handle] = useState(() => delayRender('grain'));
  useEffect(() => {
    const img = new Image();
    img.onload = () => continueRender(handle);
    img.onerror = () => cancelRender(new Error('grain tile did not load'));
    img.src = GRAIN_SRC;
  }, [handle]);

  const d = Math.max(width, height) * 0.62; // blob diameter
  const drift = (k: number, ph: number) => Math.sin(frame / (90 * k) + ph) * 36;
  const wash = (hex: string) => {
    const a = C.auroraOpacity * 1.25;
    return `radial-gradient(closest-side, ${rgba(hex, a)} 0%, ${rgba(hex, a * 0.62)} 38%, ${rgba(hex, a * 0.2)} 70%, ${rgba(hex, 0)} 100%)`;
  };
  return (
    <AbsoluteFill style={{background: C.background}}>
      <div
        style={{
          position: 'absolute',
          left: -d * 0.32 + drift(1, 0),
          top: -d * 0.22 + drift(1.3, 1),
          width: d,
          height: d,
          background: wash(C.aurora[0]),
        }}
      />
      <div
        style={{
          position: 'absolute',
          right: -d * 0.3 + drift(1.1, 2),
          bottom: -d * 0.3 + drift(0.9, 3),
          width: d,
          height: d,
          background: wash(C.aurora[1]),
        }}
      />
      <AbsoluteFill style={{backgroundImage: GRAIN, opacity: 0.05}} />
    </AbsoluteFill>
  );
};
