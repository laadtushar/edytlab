// The light, site-wide pieces. `SectionHeading` (which needs
// ScrambleText) and `Logo` are imported by path, so a page that does not
// use them does not pull them in through this barrel.
//
// `ScrollProgress` and `MicroInteractions` are not here either: they are
// loaded after first paint by `SiteMotion` (`site-motion.tsx`), and a
// re-export from this barrel is a static import of them from every page
// that uses `Reveal`, which would put them back in the first load.
export { Cascade } from "./cascade";
export { Counter } from "./counter";
export { LineDraw } from "./line-draw";
export { Magnetic } from "./magnetic";
export { Parallax } from "./parallax";
export { Reveal } from "./reveal";
export { SplitHeading } from "./split-heading";
export { Stagger } from "./stagger";
export { TiltCard } from "./tilt-card";
export { Words } from "./words";
