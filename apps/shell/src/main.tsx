import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./theme/fonts.css";
import "./theme/tokens.gen.css";
import "./theme/base.css";
import { App } from "./App";
import { tauriClient } from "./core/tauri";

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root");
createRoot(root).render(
  <StrictMode>
    <App client={tauriClient} />
  </StrictMode>,
);
