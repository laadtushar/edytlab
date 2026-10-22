import React from 'react';
import {Composition} from 'remotion';
import {CutView} from './components/CutView.tsx';
import {LAYOUTS} from './layout.ts';
import {CUTS} from './cuts.ts';
import {FPS, place} from './timeline.ts';

export const Root: React.FC = () => (
  <>
    {CUTS.map((cut) => {
      const L = LAYOUTS[cut.layout];
      return <Composition key={cut.id} id={cut.id} component={CutView} defaultProps={{cut}} durationInFrames={place(cut).total} fps={FPS} width={L.width} height={L.height} />;
    })}
  </>
);
