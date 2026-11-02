import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../theme/fonts.css";
import "../theme/tokens.gen.css";
import "../theme/base.css";
import { Gallery } from "./Gallery";

// `?component=Chip&theme=dark` shows one section alone; the full gallery otherwise.
const params = new URLSearchParams(window.location.search);
const only = { component: params.get("component"), theme: params.get("theme") };

const root = document.getElementById("root");
if (!root) throw new Error("gallery.html has no #root");
createRoot(root).render(
  <StrictMode>
    <Gallery only={only} />
  </StrictMode>,
);
