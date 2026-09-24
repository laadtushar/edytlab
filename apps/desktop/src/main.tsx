import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// The typefaces ship in the bundle (#333): a local-first editor should
// not need the network, or a third party, to draw its own name. Imported
// here rather than from styles.css, where an `@import` lands behind
// Tailwind's expanded rules and is dropped.
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "@fontsource/instrument-serif/400.css";
import "@fontsource/instrument-serif/400-italic.css";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
