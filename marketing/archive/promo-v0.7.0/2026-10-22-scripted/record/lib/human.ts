/**
 * The pointer.
 *
 * A screenshot is of the page, not the desktop, so it has no pointer in it.
 * The recorder draws one: a small arrow and a click ring, added to the page
 * by an init script and moved by the real `mousemove` and `mousedown` events
 * the shots send. It is the pointer a screen recorder would capture, drawn by
 * the recording and not by the app; it sits above the app, takes no input and
 * touches none of its elements. It is built with DOM and CSSOM calls only, so
 * the app's Content Security Policy (which the harness enforces) has nothing
 * to refuse.
 */

import type { App } from "../../../../apps/desktop/e2e/fixtures";

type Page = App["page"];

/** Real time (the machine's), for the places a shot waits on the machine and not on the page. */
export const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Call before `app.boot`: the cursor is added on every load of the page. */
export async function installCursor(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const SVG = "http://www.w3.org/2000/svg";
    const install = () => {
      if (document.getElementById("promo-cursor")) return;
      const root = document.createElement("div");
      root.id = "promo-cursor";
      const rs = root.style;
      rs.position = "fixed";
      rs.left = "0px";
      rs.top = "0px";
      rs.width = "0px";
      rs.height = "0px";
      rs.zIndex = "2147483647";
      rs.pointerEvents = "none";
      rs.display = "none";

      const arrow = document.createElementNS(SVG, "svg");
      arrow.setAttribute("width", "22");
      arrow.setAttribute("height", "26");
      arrow.setAttribute("viewBox", "0 0 22 26");
      const path = document.createElementNS(SVG, "path");
      path.setAttribute("d", "M2 2 L2 20.5 L7 16 L10.4 23.6 L13.6 22.2 L10.3 14.8 L17 14.6 Z");
      path.setAttribute("fill", "#ffffff");
      path.setAttribute("stroke", "#18181b");
      path.setAttribute("stroke-width", "1.5");
      path.setAttribute("stroke-linejoin", "round");
      arrow.appendChild(path);
      const as = arrow.style;
      as.position = "absolute";
      as.left = "-2px";
      as.top = "-2px";
      as.filter = "drop-shadow(0 1px 2px rgba(0,0,0,0.55))";
      root.appendChild(arrow);
      document.documentElement.appendChild(root);

      let seen = false;
      window.addEventListener(
        "mousemove",
        (e) => {
          if (!seen) {
            seen = true;
            rs.display = "block";
          }
          rs.transform = `translate(${e.clientX}px, ${e.clientY}px)`;
        },
        true,
      );
      window.addEventListener(
        "mousedown",
        (e) => {
          const ring = document.createElement("div");
          const s = ring.style;
          s.position = "fixed";
          s.left = `${e.clientX - 17}px`;
          s.top = `${e.clientY - 17}px`;
          s.width = "34px";
          s.height = "34px";
          s.borderRadius = "50%";
          s.border = "2px solid rgba(167, 139, 250, 0.95)";
          s.background = "rgba(167, 139, 250, 0.18)";
          s.pointerEvents = "none";
          s.zIndex = "2147483646";
          document.documentElement.appendChild(ring);
          ring.animate(
            [
              { transform: "scale(0.35)", opacity: 1 },
              { transform: "scale(1.15)", opacity: 0 },
            ],
            { duration: 420, easing: "cubic-bezier(0.16, 1, 0.3, 1)", fill: "forwards" },
          );
          // On the page's clock, which a stepped recording controls.
          setTimeout(() => ring.remove(), 440);
        },
        true,
      );
    };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", install);
    else install();
  });
}

