// What the Rust side sends and accepts. The UI holds no logic: it shows state and sends commands.
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Mode = "idle" | "teacher" | "turn" | "compare" | "mine" | "decide";
/** which phrases Back/Next/Good go through */
export type Filter = "all" | "notDone" | "marked";

export interface Snapshot {
  mode: Mode;
  phrase: number;
  phrases: number;
  listenOnly: boolean;
  filter: Filter;
  hasTake: boolean;
  note: string | null;
  done: number;
  toRepeat: number;
  tries: number;
  phraseDone: boolean;
}

export interface LessonInfo {
  id: string;
  name: string;
  group: string;
  audio: string;
  pdf: string;
  missing: boolean;
  done: number;
  total: number | null;
  /** seconds practised */
  secs: number;
  /** unix seconds last practised, 0 = never */
  last: number;
}

export interface WordView { page: number; x: number; y: number; w: number; h: number; phrase: number }
export interface PhraseView { text: string; start: number; end: number; inPdf: boolean; done: boolean }
export interface PageSize { w: number; h: number }
export interface LessonView {
  id: string;
  name: string;
  pages: PageSize[];
  words: WordView[];
  spoken: number[];
  phrases: PhraseView[];
  /** printed words marked for attention */
  marked: number[];
}

export type SessionEvent =
  | ({ type: "state" } & Snapshot)
  | { type: "word"; word: number; t: number }
  | { type: "level"; level: number; voice: boolean };

export type Out =
  | { type: "loading"; id: string; stage: string; progress: number }
  | { type: "lesson"; lesson: LessonView }
  | { type: "session"; event: SessionEvent }
  | { type: "done"; phrase: number; done: boolean }
  | { type: "micLevel"; level: number }
  | { type: "marks"; marked: number[] }
  | { type: "error"; message: string };

export type TurnEnd = "smart" | "patient" | "manual";
export type Playback = "teacherThenMe" | "meOnly" | "nothing";
export type Theme = "system" | "light" | "dark";
export interface Settings {
  input: string | null;
  output: string | null;
  turnEnd: TurnEnd;
  playback: Playback;
  theme: Theme;
  diagnostics: boolean;
  saveTakes: boolean;
}
export interface DeviceList { inputs: string[]; outputs: string[] }
export type MicState =
  | { state: "ok"; device: string }
  | { state: "blocked"; device: string }
  | { state: "missing"; error: string };

export type Cmd =
  | "space" | "good" | "again" | "prev" | "next" | "teacher" | "mine" | "stop" | "toggleListenOnly"
  | "cycleFilter"
  | { setFilter: Filter }
  | { goto: number };

const real = {
  lessons: () => invoke<LessonInfo[]>("lessons"),
  add: (paths: string[]) =>
    invoke<{ added: number; unpairedAudio: string[]; unpairedPdf: string[] }>("add_paths", { paths }),
  setPdf: (audio: string, pdf: string) => invoke<void>("set_pdf", { audio, pdf }),
  remove: (audio: string) => invoke<void>("remove_lesson", { audio }),
  open: (id: string) => invoke<void>("open_lesson", { id }),
  cmd: (cmd: Cmd) => invoke<void>("command", { cmd }),
  /** start a lesson over (done marks and tries go; marks and time practised stay) */
  resetLesson: (id: string) => invoke<void>("reset_lesson", { id }),
  clearMarks: (id: string) => invoke<void>("clear_marks", { id }),
  /** every lesson back to not done, time practised to zero (marked words stay) */
  resetAll: () => invoke<void>("reset_all"),
  /** back to a fresh install: no lessons, progress or settings (lesson files aren't touched) */
  resetApp: () => invoke<void>("reset_app"),
  /** look in the folders you added for new lessons; returns how many were added */
  rescan: () => invoke<number>("rescan"),
  removeGroup: (group: string) => invoke<void>("remove_group", { group }),
  renameGroup: (from: string, to: string) => invoke<void>("rename_group", { from, to }),
  backupProgress: (path: string) => invoke<number>("backup_progress", { path }),
  restoreProgress: (path: string) => invoke<number>("restore_progress", { path }),
  dataDir: () => invoke<string>("data_dir"),
  settings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  devices: () => invoke<DeviceList>("devices"),
  micTest: (on: boolean) => invoke<void>("mic_test", { on }),
  micCheck: () => invoke<MicState>("mic_check"),
  testSound: () => invoke<void>("test_sound"),
  exportLogs: (path: string) => invoke<number>("export_logs", { path }),
  logsDir: () => invoke<string>("logs_dir"),
  mark: (words: number[]) => invoke<void>("mark", { words }),
  markPhrase: () => invoke<void>("mark_phrase"),
  exportAnswers: (path: string) => invoke<number>("export_answers", { path }),
  onOut: (f: (o: Out) => void) => listen<Out>("ram", (e) => f(e.payload)),
  onMenu: (f: (id: string) => void) => listen<string>("menu", (e) => f(e.payload)),
};

/** In a plain browser during development, a mock stands in for the Rust side (see mock.ts). */
const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const mock = !inTauri && import.meta.env.DEV ? (await import("./mock")).mock : null;
export const api: typeof real = mock ?? real;

/** A PDF page rendered by the engine, `width` pixels wide. */
export function pageUrl(lesson: string, page: number, width: number): string {
  if (mock) return mock.pageUrl(lesson, page, width);
  return convertFileSrc(`page/${lesson}/${page}`, "ram") + `?w=${width}`;
}

export const audioExt = ["mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "mp4", "caf", "aif", "aiff"];
