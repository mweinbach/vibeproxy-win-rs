import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";

import glyphLight from "./assets/icons/light/glyph.png";
import glyphDark from "./assets/icons/dark/glyph.png";

const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

function syncFavicon() {
  const iconHref = mediaQuery.matches ? glyphDark : glyphLight;
  const existingIcon = document.querySelector("link[rel='icon']") as
    | HTMLLinkElement
    | null;

  if (existingIcon) {
    existingIcon.href = iconHref;
    existingIcon.type = "image/png";
    return;
  }

  const icon = document.createElement("link");
  icon.rel = "icon";
  icon.type = "image/png";
  icon.href = iconHref;
  document.head.appendChild(icon);
}

async function syncNativeIcons() {
  try {
    await invoke("sync_theme_icons", { isDark: mediaQuery.matches });
  } catch {
    // Ignore when running as a plain web app without a Tauri backend.
  }
}

function syncIcons() {
  syncFavicon();
  void syncNativeIcons();
}

syncIcons();
mediaQuery.addEventListener("change", syncIcons);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
