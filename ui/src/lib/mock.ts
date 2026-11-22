// A stand-in backend for working on the UI in a normal browser (`npm run dev`, no Tauri).
// Dev builds only; it replays a real lesson and fakes the session's timing. The real logic is in Rust.
import type { Cmd, LessonInfo, LessonView, MicState, Out, Settings, Snapshot } from "./api";
// ui/dev/lesson.json + page1.png: dump them from a real lesson with
//   cargo run --release --example engine_check -- <data dir> <course folder> "<lesson name>"
// (they're your course material, so they're not in git)
const files = import.meta.glob("../../dev/*", { eager: true, import: "default" }) as Record<string, unknown>;
const sample = files["../../dev/lesson.json"] as Omit<LessonView, "marked"> | undefined;
const lesson: LessonView = {
  marked: [],
  ...(sample ?? {
    id: "none", name: "No sample lesson (see mock.ts)", pages: [{ w: 612, h: 792 }], words: [], spoken: [],
    phrases: [{ text: "Put a sample lesson in ui/dev to try the UI here.", start: 0, end: 1, inPdf: true, done: false }, { text: "Second phrase", start: 2, end: 3, inPdf: true, done: false }],
  }),
};
const pageImg = (files["../../dev/page1.png"] as string | undefined) ?? "";
let listeners: ((o: Out) => void)[] = [];
const emit = (o: Out) => listeners.forEach((f) => f(o));
let timers: ReturnType<typeof setTimeout>[] = [];
const later = (ms: number, f: () => void) => timers.push(setTimeout(f, ms));
const clear = () => { timers.forEach(clearTimeout); timers = []; };

const lessons: LessonInfo[] = [
  { id: lesson.id, name: lesson.name, group: "L4 - R", audio: "/c/R Sentences.m4a", pdf: "/c/R Sentences.pdf", missing: false, done: 3, total: 21, secs: 1260, last: Math.floor(Date.now() / 1000) - 86400 },
  { id: "other", name: "R", group: "L4 - R", audio: "/c/R.m4a", pdf: "/c/R.pdf", missing: false, done: 0, total: null, secs: 0, last: 0 },
  { id: "gone", name: "Lingering", group: "L5 - Lingering", audio: "/c/Lingering.m4a", pdf: "/c/Lingering.pdf", missing: true, done: 0, total: null, secs: 0, last: 0 },
];

const snap: Snapshot = { mode: "idle", phrase: 1, phrases: lesson.phrases.length, listenOnly: false, filter: "all", hasTake: false, note: null, done: 0, toRepeat: lesson.phrases.filter((p) => p.inPdf).length, tries: 0, phraseDone: false };
const state = (patch: Partial<Snapshot>) => { Object.assign(snap, patch); emit({ type: "session", event: { type: "state", ...snap } }); };

function wordsOf(k: number): number[] {
  // spoken words whose printed word belongs to phrase k
  return lesson.spoken.map((pdf, i) => [pdf, i]).filter(([pdf]) => pdf >= 0 && lesson.words[pdf].phrase === k).map(([, i]) => i);
}

function teacher(k: number, then: () => void) {
  clear();
  state({ mode: "teacher", phrase: k, note: null });
  const ws = wordsOf(k);
  const dur = Math.max(800, (lesson.phrases[k].end - lesson.phrases[k].start) * 1000);
  ws.forEach((w, i) => later((dur * i) / ws.length, () => emit({ type: "session", event: { type: "word", word: w, t: 0 } })));
  later(dur, () => { emit({ type: "session", event: { type: "word", word: -1, t: 0 } }); then(); });
}

function turn() {
  state({ mode: "turn" });
  for (let i = 0; i < 40; i++) later(i * 50, () => emit({ type: "session", event: { type: "level", level: i > 6 && i < 30 ? 0.4 + Math.random() * 0.5 : 0.05, voice: i > 6 && i < 30 } }));
  later(2000, finishTurn);
}

function finishTurn() {
  clear();
  state({ hasTake: true, tries: snap.tries + 1 });
  compare();
}

function compare() {
  clear();
  state({ mode: "compare" });
  later(2200, () => state({ mode: "decide" }));
}

const next = (k: number) => { for (let i = k + 1; i < lesson.phrases.length; i++) if (snap.listenOnly || lesson.phrases[i].inPdf) return i; return -1; };
const prev = (k: number) => { for (let i = k - 1; i >= 0; i--) if (snap.listenOnly || lesson.phrases[i].inPdf) return i; return -1; };

function practise(k: number) {
  state({ hasTake: false, tries: 0, phraseDone: lesson.phrases[k].done });
  teacher(k, () => (snap.listenOnly ? (next(k) >= 0 ? later(500, () => practise(next(k))) : state({ mode: "idle" })) : turn()));
}

function command(c: Cmd) {
  if (typeof c === "object") return "goto" in c ? practise(c.goto) : state({ filter: c.setFilter });
  switch (c) {
    case "space":
      if (snap.mode === "idle") practise(snap.phrase);
      else if (snap.mode === "teacher") { clear(); snap.listenOnly ? state({ mode: "idle" }) : turn(); }
      else if (snap.mode === "turn") finishTurn();
      else if (snap.mode === "compare" || snap.mode === "mine") { clear(); state({ mode: "decide" }); }
      else if (snap.mode === "decide") snap.hasTake ? compare() : practise(snap.phrase);
      break;
    case "good": {
      lesson.phrases[snap.phrase].done = true;
      emit({ type: "done", phrase: snap.phrase, done: true });
      snap.done += 1;
      const n = next(snap.phrase);
      if (n >= 0) practise(n); else { clear(); state({ mode: "idle", note: "Lesson done!" }); }
      break;
    }
    case "again": teacher(snap.phrase, turn); break;
    case "next": { const n = next(snap.phrase); if (n >= 0) practise(n); break; }
    case "prev": { const n = prev(snap.phrase); if (n >= 0) practise(n); break; }
    case "teacher": teacher(snap.phrase, () => state({ mode: snap.hasTake ? "decide" : "idle" })); break;
    case "mine": if (snap.hasTake) { clear(); state({ mode: "mine" }); later(1500, () => state({ mode: "decide" })); } else state({ note: "No take of this phrase yet." }); break;
    case "stop": clear(); state({ mode: "idle" }); break;
    case "toggleListenOnly": state({ listenOnly: !snap.listenOnly }); break;
    case "cycleFilter": state({ filter: snap.filter === "all" ? "notDone" : snap.filter === "notDone" ? "marked" : "all" }); break;
  }
}

let settings: Settings = { input: null, output: null, turnEnd: "smart", playback: "teacherThenMe", theme: "system", diagnostics: false, saveTakes: true };
let marked = new Set<number>();
const marksOut = () => emit({ type: "marks", marked: [...marked] });
let micTimer: ReturnType<typeof setInterval> | undefined;

let checks = 0; // ?blocked: the first two checks say "blocked", like a fresh install

export const mock = {
  testSound: async () => {},
  mark: async (w: number[]) => { const all = w.every((i) => marked.has(i)); w.forEach((i) => (all ? marked.delete(i) : marked.add(i))); marksOut(); },
  markPhrase: async () => { const w = lesson.words.map((x, i) => [x, i] as const).filter(([x]) => x.phrase === snap.phrase).map(([, i]) => i); const all = w.every((i) => marked.has(i)); w.forEach((i) => (all ? marked.delete(i) : marked.add(i))); marksOut(); },
  exportAnswers: async (_p: string) => 4,
  exportLogs: async (_p: string) => 3,
  logsDir: async () => "/mock/logs",
  micCheck: async (): Promise<MicState> => {
    await new Promise((r) => setTimeout(r, 300));
    checks++;
    return new URLSearchParams(location.search).has("blocked") && checks < 3
      ? { state: "blocked", device: "MacBook Pro Microphone" }
      : { state: "ok", device: "MacBook Pro Microphone" };
  },
  settings: async () => settings,
  setSettings: async (s: Settings) => { settings = s; },
  devices: async () => ({ inputs: ["MacBook Pro Microphone", "AirPods Pro"], outputs: ["MacBook Pro Speakers", "AirPods Pro"] }),
  micTest: async (on: boolean) => {
    clearInterval(micTimer);
    if (on) micTimer = setInterval(() => emit({ type: "micLevel", level: Math.random() * 0.8 }), 60);
  },
  lessons: async () => (new URLSearchParams(location.search).has("empty") ? [] : lessons),
  add: async (_paths: string[]) => ({ added: 0, unpairedAudio: [], unpairedPdf: [] }),
  setPdf: async () => {},
  resetLesson: async (_id: string) => {},
  clearMarks: async (_id: string) => { marked.clear(); marksOut(); },
  resetAll: async () => {},
  resetApp: async () => {},
  rescan: async () => 0,
  removeGroup: async (_g: string) => {},
  renameGroup: async (_from: string, _to: string) => {},
  backupProgress: async (_p: string) => lessons.length,
  restoreProgress: async (_p: string) => 1,
  remove: async () => {},
  open: async (id: string) => {
    if (new URLSearchParams(location.search).has("empty")) return;
    clear();
    emit({ type: "loading", id, stage: "Getting the lesson ready", progress: 0.3 });
    if (id !== lesson.id) { later(700, () => emit({ type: "error", message: "The mock has only one lesson." })); return; }
    later(500, () => { emit({ type: "lesson", lesson }); state({ mode: "idle", phrase: 1 }); });
  },
  cmd: async (c: Cmd) => command(c),
  dataDir: async () => "/mock",
  onOut: async (f: (o: Out) => void) => { listeners.push(f); return () => { listeners = listeners.filter((x) => x !== f); }; },
  onMenu: async (_f: (id: string) => void) => () => {},
  pageUrl: (_l: string, _p: number, _w: number) => pageImg,
};
