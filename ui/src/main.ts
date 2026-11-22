import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { invoke } from "@tauri-apps/api/core";

// macOS: the window buttons float over the top-left corner (hidden title bar)
if ("__TAURI_INTERNALS__" in window && navigator.userAgent.includes("Mac")) document.documentElement.classList.add("mac-frame");

// any error in the window goes to logs/app.log (there's no console in the installed app)
if ("__TAURI_INTERNALS__" in window) {
  const report = (m: string) => invoke("log_ui", { message: m.slice(0, 2000) }).catch(() => {});
  window.addEventListener("error", (e) => report(`${e.message} at ${e.filename}:${e.lineno}`));
  window.addEventListener("unhandledrejection", (e) => report(`unhandled: ${String(e.reason?.stack ?? e.reason)}`));
}

mount(App, { target: document.getElementById("app")! });
