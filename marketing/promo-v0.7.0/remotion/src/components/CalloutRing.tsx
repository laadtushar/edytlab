import React from 'react';
import {spring, useVideoConfig} from 'remotion';
import {C, clamp01, easeIn, measureText, rgba} from '../brand.ts';
import {toCard, type Cam} from '../camera.ts';
import type {Box, Layout} from '../layout.ts';
import type {Label, Ring, Side} from '../timeline.ts';
import {Chip, chipPad} from './Chip.tsx';
import {GuardedDiv} from './SafeCheck.tsx';

/** A ring span in shot time, with its labels in shot time. */
export interface PlacedRing {
  ring: Ring;
  t0: number;
  t1: number;
  labels: Label[];
}

const PAD = 10; // card px between the rectangle and its ring
const GAP = 14; // between the ring and its label
const EDGE = 18; // a label stays this far inside the card

const intersects = (a: Box, b: Box) => a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;

function labelBox(anchor: Box, side: Side, w: number, h: number): Box {
  switch (side) {
    case 'above':
      return {x: anchor.x + anchor.w / 2 - w / 2, y: anchor.y - GAP - h, w, h};
    case 'below':
      return {x: anchor.x + anchor.w / 2 - w / 2, y: anchor.y + anchor.h + GAP, w, h};
    case 'left':
      return {x: anchor.x - GAP - w, y: anchor.y + anchor.h / 2 - h / 2, w, h};
    case 'right':
      return {x: anchor.x + anchor.w + GAP, y: anchor.y + anchor.h / 2 - h / 2, w, h};
  }
}

const fits = (b: Box, card: Box) => b.x >= EDGE && b.y >= EDGE && b.x + b.w <= card.w - EDGE && b.y + b.h <= card.h - EDGE;

/**
 * A callout ring over a rectangle of the app window, drawn on and breathing, with its label in a chip beside it. The
 * rectangle is in window CSS px and goes through the camera, so the ring follows a moving shot. The label is placed on
 * the first side it fits inside the card, else clamped inside it, so it is always inside the card (and so the safe area).
 */
export const CalloutRing: React.FC<{layout: Layout; cam: Cam; frame: number; placed: PlacedRing}> = ({layout, cam, frame, placed}) => {
  const {fps} = useVideoConfig();
  const {card} = layout;
  const {ring} = placed;
  const t = frame / fps;
  const age = t - placed.t0;
  const left = placed.t1 - t;
  if (age < 0 || left < 0) return null;
  const appear = clamp01(age / 0.4);
  const exit = clamp01(1 - left / 0.25);
  const alpha = clamp01(appear * 1.3) * (1 - easeIn(exit));
  const r = toCard(cam, card, ring.rect);
  const box: Box = {x: r.x - PAD, y: r.y - PAD, w: r.w + 2 * PAD, h: r.h + 2 * PAD};
  const cardLocal: Box = {x: 0, y: 0, w: card.w, h: card.h};
  if (!intersects(box, cardLocal)) return null;

  const stroke = Math.max(3, Math.round(layout.labelSize * 0.12));
  const radius = Math.min(26, 10 + Math.min(box.w, box.h) * 0.12);
  const perim = 2 * (box.w + box.h);
  const draw = spring({frame: frame - Math.round(placed.t0 * fps), fps, config: {damping: 22, stiffness: 120, mass: 0.8}});
  const pulse = 0.5 + 0.5 * Math.sin(t * 3.1);
  const burst = clamp01(age / 0.5);
  const tone = ring.tone ?? 'violet';

  const text = placed.labels.find((l) => t >= l.t0 && t < l.t1)?.text;
  let labelEl: React.ReactNode = null;
  if (text) {
    const size = layout.labelSize;
    const pad = chipPad(size);
    const w = Math.ceil(measureText(text, 600, size) + 2 * pad.x + 2 * Math.max(2, size * 0.07) + size * 0.62) + 3;
    const h = Math.ceil(size + 2 * pad.y + 2 * Math.max(2, size * 0.07));
    // the part of the ring that is in the card, so a ring running off an edge still has a label beside what is seen
    const vis: Box = {
      x: Math.max(box.x, 0),
      y: Math.max(box.y, 0),
      w: Math.min(box.x + box.w, card.w) - Math.max(box.x, 0),
      h: Math.min(box.y + box.h, card.h) - Math.max(box.y, 0),
    };
    const prefer: Side[] = ring.side ? [ring.side] : ['above', 'below', 'right', 'left'];
    const order: Side[] = [...prefer, ...(['above', 'below', 'right', 'left'] as Side[]).filter((s) => !prefer.includes(s))];
    const found = order.map((s) => labelBox(vis, s, w, h)).find((b) => fits(b, cardLocal));
    const b = found ?? labelBox(vis, order[0], w, h);
    const lb = {...b, x: Math.min(Math.max(b.x, EDGE), card.w - EDGE - w), y: Math.min(Math.max(b.y, EDGE), card.h - EDGE - h)};
    const labelIn = spring({frame: frame - Math.round((placed.t0 + 0.12) * fps), fps, config: {damping: 16, stiffness: 170, mass: 0.6}});
    labelEl = (
      <GuardedDiv
        layout={layout}
        what={`label "${text}"`}
        style={{position: 'absolute', left: lb.x, top: lb.y, width: w, height: h, opacity: clamp01(labelIn * 1.5) * (1 - easeIn(exit)), transform: `translateY(${(1 - labelIn) * 12}px) scale(${0.94 + 0.06 * labelIn})`, transformOrigin: '50% 50%'}}
      >
        <Chip text={text} size={size} tone={tone} dot />
      </GuardedDiv>
    );
  }

  return (
    <>
      {ring.noRing ? null : (
        <svg style={{position: 'absolute', left: 0, top: 0, width: card.w, height: card.h, overflow: 'visible', opacity: alpha}} viewBox={`0 0 ${card.w} ${card.h}`}>
          <rect x={box.x - 8 * burst} y={box.y - 8 * burst} width={box.w + 16 * burst} height={box.h + 16 * burst} rx={radius + 6 * burst} fill="none" stroke={rgba(C.primary, 0.5 * (1 - burst))} strokeWidth={stroke} />
          <rect x={box.x} y={box.y} width={box.w} height={box.h} rx={radius} fill={rgba(C.primary, 0.07 + 0.04 * pulse)} stroke="none" />
          <rect
            x={box.x}
            y={box.y}
            width={box.w}
            height={box.h}
            rx={radius}
            fill="none"
            stroke={C.primary}
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={perim}
            strokeDashoffset={perim * (1 - clamp01(draw))}
            style={{filter: `drop-shadow(0 0 ${8 + 6 * pulse}px ${rgba(C.primary, 0.75)})`}}
          />
        </svg>
      )}
      {labelEl}
    </>
  );
};
