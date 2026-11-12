import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./styles/tokens.css";
import { applyTheme } from "./theme/theme";

applyTheme();
// A soft keyboard shrinks only the visual viewport. The app follows it, so the terminal refits above
// the keyboard (its ResizeObserver sends the resize frame); times scale, so a pinch-zoom changes nothing.
// A browser that scrolled the page to show the caret (iOS) is scrolled back: the app is all on screen.
// Not while pinch-zoomed: that would snap every zoom step back to the corner.
const vv = window.visualViewport;
vv?.addEventListener("resize", () => {
  document.documentElement.style.height = `${String(vv.height * vv.scale)}px`;
  if (vv.scale === 1) scrollTo(0, 0);
});
const root = document.getElementById("root");
if (root)
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
