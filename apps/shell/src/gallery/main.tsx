import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../theme/fonts.css";
import "../theme/tokens.gen.css";
import "../theme/base.css";
import { Gallery } from "./Gallery";

const root = document.getElementById("root");
if (!root) throw new Error("gallery.html has no #root");
createRoot(root).render(
  <StrictMode>
    <Gallery />
  </StrictMode>,
);
