// Every shortcut, in one place: the key handler, the buttons' key labels, the shortcut bar and the
// help sheet all read this table, so what you see is always what works.
import type { Cmd, Mode, Snapshot } from "./api";

export const isMac = navigator.platform.toLowerCase().includes("mac") || navigator.userAgent.includes("Mac");
export const mod = isMac ? "⌘" : "Ctrl";

export type Action =
  | "space" | "good" | "again" | "prev" | "next" | "teacher" | "mine" | "stop" | "listenOnly" | "filter" | "mark"
  | "reset" | "rescan" | "help" | "lessons" | "settings" | "addFiles" | "addFolder" | "zoomIn" | "zoomOut" | "zoomFit";

export interface Key {
  action: Action;
  /** physical keys (KeyboardEvent.code): the same on every keyboard layout (Ukrainian, Polish, …) */
  codes: string[];
  /** typed characters that also count (e.g. "?" wherever it is) */
  keys: string[];
  /** what the keycap shows */
  cap: string;
  /** modifier needed (⌘ on Mac, Ctrl elsewhere) */
  mod?: boolean;
  shift?: boolean;
  label: string;
  cmd?: Cmd;
}

export const KEYS: Key[] = [
  { action: "space", codes: ["Space"], keys: [" "], cap: "Space", label: "Play / pause", cmd: "space" },
  { action: "good", codes: ["Enter", "NumpadEnter"], keys: ["Enter"], cap: "↵", label: "Good: next phrase", cmd: "good" },
  { action: "again", codes: ["KeyR"], keys: [], cap: "R", label: "Again", cmd: "again" },
  { action: "next", codes: ["KeyN", "ArrowRight"], keys: [], cap: "N", label: "Next phrase (also →)", cmd: "next" },
  { action: "prev", codes: ["ArrowLeft", "KeyB", "Escape"], keys: [], cap: "B", label: "Back: previous phrase (also ← or Esc)", cmd: "prev" },
  { action: "teacher", codes: ["KeyT"], keys: [], cap: "T", label: "Teacher only", cmd: "teacher" },
  { action: "mine", codes: ["KeyM"], keys: [], cap: "M", label: "My take only", cmd: "mine" },
  { action: "listenOnly", codes: ["KeyL"], keys: [], cap: "L", label: "Listen-only mode", cmd: "toggleListenOnly" },
  { action: "filter", codes: ["KeyO"], keys: [], cap: "O", label: "Which phrases: all · not done · marked", cmd: "cycleFilter" },
  { action: "mark", codes: ["KeyF"], keys: [], cap: "F", label: "Mark the phrase's words for attention" },
  { action: "stop", codes: ["KeyS"], keys: [], cap: "S", label: "Stop", cmd: "stop" },
  { action: "help", codes: ["Slash"], keys: ["?"], cap: "?", label: "All shortcuts" },
  { action: "lessons", codes: ["KeyL"], keys: [], cap: "L", mod: true, label: "Lessons" },
  { action: "settings", codes: ["Comma"], keys: [], cap: ",", mod: true, label: "Settings" },
  { action: "addFiles", codes: ["KeyO"], keys: [], cap: "O", mod: true, label: "Add lesson files" },
  { action: "addFolder", codes: ["KeyO"], keys: [], cap: "O", mod: true, shift: true, label: "Add a folder" },
  { action: "rescan", codes: ["KeyR"], keys: [], cap: "R", mod: true, label: "Look for new lessons in your folders" },
  { action: "zoomIn", codes: ["Equal", "NumpadAdd"], keys: ["+"], cap: "+", mod: true, label: "Zoom in" },
  { action: "zoomOut", codes: ["Minus", "NumpadSubtract"], keys: [], cap: "−", mod: true, label: "Zoom out" },
  { action: "zoomFit", codes: ["Digit0", "Numpad0"], keys: [], cap: "0", mod: true, label: "Fit width" },
];

export function keyOf(a: Action): Key {
  return KEYS.find((k) => k.action === a)!;
}

export function capText(k: Key): string {
  return (k.mod ? mod + (isMac ? "" : "+") : "") + (k.shift ? (isMac ? "⇧" : "Shift+") : "") + k.cap;
}

/** Which key an event means, if any. */
export function match(e: KeyboardEvent): Key | undefined {
  const m = isMac ? e.metaKey : e.ctrlKey;
  return KEYS.find((k) => {
    if (!k.codes.includes(e.code) && !k.keys.includes(e.key)) return false;
    if (!!k.mod !== m) return false;
    // with ⌘/Ctrl, Shift picks a different shortcut (⌘O vs ⌘⇧O); plain keys work with or without Shift
    if (k.mod && !!k.shift !== e.shiftKey) return false;
    if (!k.mod && (e.altKey || e.ctrlKey || e.metaKey)) return false;
    return true;
  });
}

/** What Space does right now, in words. */
export function spaceLabel(s: Snapshot | null): string {
  if (!s) return "Start";
  switch (s.mode) {
    case "idle": return s.listenOnly ? "Play" : "Start";
    case "teacher": return s.listenOnly ? "Pause" : "My turn now";
    case "turn": return "Done talking";
    case "compare":
    case "mine": return "Skip";
    case "decide": return s.hasTake ? "Hear both again" : "Try";
  }
}

/** A screen on top (onboarding, the add sheet) takes keys first: it returns true for a key it used. */
type Layer = (e: KeyboardEvent) => boolean;
const layers: Layer[] = [];

/** Use in onMount: `onMount(() => keyLayer(handler))`, which also removes it again. */
export function keyLayer(h: Layer): () => void {
  layers.push(h);
  return () => {
    const i = layers.indexOf(h);
    if (i >= 0) layers.splice(i, 1);
  };
}

/** The newest layer first. True when a layer used the key. */
export function layerKey(e: KeyboardEvent): boolean {
  if (e.metaKey || e.ctrlKey || e.altKey) return false;
  for (let i = layers.length - 1; i >= 0; i--) if (layers[i](e)) return true;
  return false;
}

/** Enter or Space on a focused button presses that button, as in any app. */
export function pressFocused(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if ((e.key === "Enter" || e.key === " ") && t?.tagName === "BUTTON") {
    (t as HTMLButtonElement).click();
    return true;
  }
  return false;
}
