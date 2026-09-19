import "./three-setup";
import "@fontsource/b612/400.css";
import "@fontsource/b612/700.css";
import "@fontsource/b612-mono/400.css";
import "@fontsource/b612-mono/700.css";
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
