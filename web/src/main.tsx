import "./three-setup";
// Bundled, not linked: the console runs on field networks with no internet. Latin subset covers − — ≈ ° ².
import "@fontsource/montserrat/latin-400.css";
import "@fontsource/montserrat/latin-500.css";
import "@fontsource/montserrat/latin-600.css";
import "@fontsource/montserrat/latin-700.css";
import "uplot/dist/uPlot.min.css";
import "./styles/tokens.css";
import "./styles/app.css";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { connect, sendCommand } from "./ws";
import { startTick } from "./store/ui";

// Dev-only hook so scripts/smoke.mjs can send a command the UI would have interlocked.
if (import.meta.env.DEV) (window as unknown as { __gs: unknown }).__gs = { sendCommand };

connect();
startTick();
createRoot(document.getElementById("root")!).render(<App />);
