<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog, ask, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { api, type LessonInfo, type LessonView, type Settings as SettingsT, type Snapshot } from "./lib/api";
  import { capText, keyOf, layerKey, match, type Action } from "./lib/keys";
  import PdfView from "./components/PdfView.svelte";
  import PracticePanel from "./components/PracticePanel.svelte";
  import LessonDrawer from "./components/LessonDrawer.svelte";
  import Help from "./components/Help.svelte";
  import Welcome from "./components/Welcome.svelte";
  import Settings from "./components/Settings.svelte";
  import AddLesson from "./components/AddLesson.svelte";

  let lessons = $state<LessonInfo[]>([]);
  let lesson = $state<LessonView | null>(null);
  let snap = $state<Snapshot | null>(null);
  let word = $state(-1); // spoken word being said
  let level = $state(0);
  let voice = $state(false);
  let loading = $state<{ id: string; stage: string; progress: number } | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let drawer = $state(false);
  let help = $state(false);
  let zoom = $state(1);
  let dropping = $state(false);
  /** what's being added right now (shown while a folder or zip is read) */
  let importing = $state<string | null>(null);
  let currentId = $state<string | null>(null);
  let started = $state(false);
  let settingsOpen = $state(false);
  let settings = $state<SettingsT | null>(null);
  let micLevel = $state(0);
  let addOpen = $state(false);
  let dropped = $state<string[] | null>(null);

  // the theme setting: System follows the OS; Light/Dark pin it
  $effect(() => {
    const t = settings?.theme ?? "system";
    if (t === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = t;
  });

  // confirmations close by themselves; errors stay until dismissed
  $effect(() => {
    if (!notice) return;
    const t = setTimeout(() => (notice = null), 4500);
    return () => clearTimeout(t);
  });

  async function exportAnswers() {
    if (!lesson) return;
    const path = await saveDialog({
      title: "Save your answers for your teacher",
      defaultPath: `${lesson.name} - my answers.wav`,
      filters: [{ name: "Audio (WAV)", extensions: ["wav"] }],
    });
    if (!path) return;
    try {
      const n = await api.exportAnswers(path);
      notice = `Saved ${n} answer${n === 1 ? "" : "s"}: the teacher, then you, phrase by phrase.`;
    } catch (e) {
      error = String(e);
    }
  }

  function changeSettings(s: SettingsT) {
    settings = s;
    api.setSettings(s).catch((e) => (error = String(e)));
  }

  const LAST = "ram.lastLesson";
  const macFrame = "__TAURI_INTERNALS__" in window && navigator.userAgent.includes("Mac");

  async function refresh() {
    lessons = await api.lessons();
  }

  function openLesson(id: string) {
    if (!id) return;
    currentId = id;
    drawer = false;
    error = null;
    loading = { id, stage: "Opening", progress: 0 };
    snap = null;
    try { localStorage.setItem(LAST, id); } catch {}
    api.open(id);
  }

  async function addPaths(paths: string[]) {
    if (!paths.length) return;
    const name = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
    const what = paths.length === 1 ? `“${name(paths[0])}”` : `${paths.length} items`;
    importing = paths.some((p) => /\.zip$/i.test(p)) ? `Unpacking and adding ${what}…` : `Adding ${what}…`;
    try {
      const r = await api.add(paths);
      await refresh();
      const parts: string[] = [];
      parts.push(r.added === 0 ? `No new lessons in ${what}.` : r.added === 1 ? `Added 1 lesson from ${what}.` : `Added ${r.added} lessons from ${what}.`);
      if (r.unpairedAudio.length)
        parts.push(`${r.unpairedAudio.length} recording${r.unpairedAudio.length > 1 ? "s have" : " has"} no matching PDF. Add the PDF with it, or pick one in the lesson list.`);
      notice = parts.join(" ");
      if (r.added > 0) {
        // show what came in; with no lesson open yet, start on the first one
        if (!lesson) {
          const first = lessons.find((l) => !l.missing);
          if (first) openLesson(first.id);
        }
        drawer = true;
      }
    } catch (e) {
      error = String(e);
    } finally {
      importing = null;
    }
  }

  function addFiles() {
    drawer = false;
    if (lessons.length === 0) return; // onboarding already shows the two slots
    dropped = null;
    addOpen = true;
  }

  async function addFolder() {
    const picked = await openDialog({ directory: true, multiple: false, title: "Choose a folder with lessons" });
    if (picked) addPaths([picked as string]);
  }

  function confirmReset(target?: { id: string; name: string }) {
    const l = target ?? (lesson ? { id: lesson.id, name: lesson.name } : null);
    if (!l) return;
    ask(`Every phrase in "${l.name}" goes back to not done. Your marked words and time practised stay.`, { title: "Start this lesson over?", kind: "warning", okLabel: "Start Over" })
      .then(async (yes) => {
        if (!yes) return;
        await api.resetLesson(l.id);
        refresh();
      });
  }

  // look for lessons added to your folders since last time
  async function rescan(say = false) {
    const n = await api.rescan().catch(() => 0);
    if (n > 0 || say) {
      await refresh();
      notice = n > 0 ? `Found ${n} new lesson${n === 1 ? "" : "s"} in your folders.` : "No new lessons in your folders.";
    }
  }

  // the app was reset: back to the welcome screen, as on first launch
  async function afterReset() {
    settingsOpen = false;
    drawer = false;
    lesson = null;
    snap = null;
    currentId = null;
    loading = null;
    try { localStorage.removeItem(LAST); } catch {}
    settings = await api.settings();
    await refresh();
    notice = "The app is reset. Add a folder of lessons to start again.";
  }

  async function backupProgress() {
    const d = new Date().toISOString().slice(0, 10);
    const path = await saveDialog({ title: "Back up your progress", defaultPath: `Repeat After Me progress ${d}.json`, filters: [{ name: "Progress", extensions: ["json"] }] });
    if (!path) return;
    try {
      const n = await api.backupProgress(path);
      notice = `Saved the progress of ${n} lesson${n === 1 ? "" : "s"}.`;
    } catch (e) {
      error = String(e);
    }
  }

  async function restoreProgress() {
    const path = await openDialog({ title: "Restore progress from a backup", filters: [{ name: "Progress", extensions: ["json"] }] });
    if (!path) return;
    try {
      const n = await api.restoreProgress(path as string);
      await refresh();
      notice = `Restored the progress of ${n} lesson${n === 1 ? "" : "s"}, added to what's here (nothing is lost).`;
    } catch (e) {
      error = String(e);
    }
  }

  function act(a: Action) {
    const playing = lesson && snap;
    switch (a) {
      case "help": help = !help; return;
      case "lessons": drawer = !drawer; if (drawer) refresh(); return;
      case "settings": settingsOpen = !settingsOpen; return;
      case "mark": if (lesson) api.markPhrase(); return;
      case "reset": confirmReset(); return;
      case "rescan": rescan(true); return;
      case "addFiles": addFiles(); return;
      case "addFolder": addFolder(); return;
      case "zoomIn": zoom = Math.min(3, +(zoom * 1.15).toFixed(2)); return;
      case "zoomOut": zoom = Math.max(0.5, +(zoom / 1.15).toFixed(2)); return;
      case "zoomFit": zoom = 1; return;
    }
    if (!playing) return;
    const cmd = {
      space: "space", good: "good", again: "again", prev: "prev", next: "next",
      teacher: "teacher", mine: "mine", stop: "stop", listenOnly: "toggleListenOnly", filter: "cycleFilter",
    } as const;
    api.cmd(cmd[a as keyof typeof cmd]);
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) return;
    if (e.key === "Escape" && (help || drawer || settingsOpen || addOpen)) {
      help = false;
      drawer = false;
      settingsOpen = false;
      addOpen = false;
      e.preventDefault();
      return;
    }
    // onboarding and the add sheet take their keys first
    if (layerKey(e)) {
      e.preventDefault();
      return;
    }
    if (addOpen) return; // no lesson keys behind the sheet
    const k = match(e);
    if (!k) return;
    e.preventDefault(); // Space must never scroll or press a focused button
    if (e.repeat) return;
    act(k.action);
  }

  function gotoPhrase(k: number) {
    api.cmd({ goto: k });
  }

  async function reveal() {
    const dir = await api.dataDir();
    revealItemInDir(dir + "/progress.json").catch(() => revealItemInDir(dir));
  }

  onMount(() => {
    const offs: (() => void)[] = [];
    api.onOut((o) => {
      switch (o.type) {
        case "loading":
          if (o.id === currentId) loading = o;
          break;
        case "lesson":
          lesson = o.lesson;
          loading = null;
          word = -1;
          break;
        case "session": {
          const ev = o.event;
          if (ev.type === "state") {
            const { type: _t, ...s } = ev;
            snap = s;
            if (s.mode !== "turn") { level = 0; voice = false; }
            if (s.mode === "idle" || s.mode === "decide" || s.mode === "turn") word = -1;
          } else if (ev.type === "word") {
            word = ev.word;
          } else if (ev.type === "level") {
            level = ev.level;
            voice = ev.voice;
          }
          break;
        }
        case "done":
          if (lesson) lesson.phrases[o.phrase].done = o.done;
          break;
        case "marks":
          if (lesson) lesson.marked = o.marked;
          break;
        case "micLevel":
          micLevel = o.level;
          break;
        case "error":
          error = o.message;
          loading = null;
          break;
      }
    }).then((f) => offs.push(f));
    api.onMenu((id) => {
      const map: Record<string, Action> = {
        "add-files": "addFiles", "add-folder": "addFolder", keys: "help", lessons: "lessons", settings: "settings",
        "zoom-in": "zoomIn", "zoom-out": "zoomOut", "zoom-fit": "zoomFit",
      };
      if (map[id]) act(map[id]);
      else if (id === "data") reveal();
      else if (id === "reset") confirmReset();
      else if (id === "backup") backupProgress();
      else if (id === "restore") restoreProgress();
      else if (id === "rescan") rescan(true);
    }).then((f) => offs.push(f));
    if ("__TAURI_INTERNALS__" in window) getCurrentWebview().onDragDropEvent((ev) => {
      const p = ev.payload;
      if (p.type === "enter" || p.type === "over") dropping = true;
      else if (p.type === "leave") dropping = false;
      else if (p.type === "drop") {
        dropping = false;
        // a folder or a zip is added right away; loose files go into the audio + PDF slots
        const files = p.paths.filter((x) => /\.[a-z0-9]{2,5}$/i.test(x) && !/\.zip$/i.test(x));
        if (files.length < p.paths.length) addPaths(p.paths);
        else if (files.length === 2 && !addOpen && lessons.length > 0) addPaths(p.paths);
        else {
          dropped = [...p.paths];
          if (lessons.length > 0) addOpen = true;
        }
      }
    }).then((f) => offs.push(f));

    api.settings().then((s) => (settings = s));
    rescan();
    refresh().then(() => {
      started = true;
      let last: string | null = null;
      try { last = localStorage.getItem(LAST); } catch {}
      const pick = lessons.find((l) => l.id === last && !l.missing) ?? lessons.find((l) => !l.missing);
      if (pick) openLesson(pick.id);
    });
    return () => offs.forEach((f) => f());
  });

  const current = $derived(lessons.find((l) => l.id === currentId));
  const phrase = $derived(lesson && snap ? lesson.phrases[snap.phrase] : null);
  // Mark works on the current phrase's printed words: none (the teacher explaining) = nothing to mark
  const canMark = $derived(!!lesson && !!snap && lesson.words.some((w) => w.phrase === snap!.phrase));
  const phraseMarked = $derived.by(() => {
    if (!lesson || !snap) return false;
    const m = new Set(lesson.marked);
    return lesson.words.some((w, i) => w.phrase === snap!.phrase && m.has(i));
  });
</script>

<svelte:window onkeydown={onKey} />

<!-- empty background anywhere (the strip above the header, gaps between cards) moves the window -->
<div class="app" class:dropping data-tauri-drag-region>
  <!-- on macOS the window's title bar is hidden: this header holds the traffic lights and drags the window -->
  <header data-tauri-drag-region class:mac-frame={macFrame}>
    <!-- the same button opens and closes the lesson list: while it's open it sits above the list, as ✕ -->
    <button class="btn lessons-btn" class:over={drawer} onclick={() => act("lessons")} title={drawer ? "Close the lesson list" : "Lessons"}>
      <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true">
        {#if drawer}
          <path d="M6 6l12 12M18 6L6 18" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
        {:else}
          <path d="M4 6h16M4 12h16M4 18h16" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
        {/if}
      </svg>
      <span class="hide-narrow">Lessons</span>
      <span class="kbd hide-narrow">{capText(keyOf("lessons"))}</span>
    </button>
    <!-- title + progress: inline in the header; on a phone-width window, their own row under the buttons -->
    <div class="info" data-tauri-drag-region>
    <div class="title" data-tauri-drag-region>
      {#if lesson}
        <strong>{lesson.name}</strong>
        {#if current?.group}<span class="muted hide-narrow"> · {current.group}</span>{/if}
      {:else}
        <strong>Repeat After Me</strong>
      {/if}
    </div>
    {#if snap && snap.toRepeat > 0}
      <div class="progress" title="Phrases you called good" data-tauri-drag-region>
        <span class="count">{snap.done}<span class="muted">&thinsp;/&thinsp;{snap.toRepeat}</span></span>
        <div class="bar"><div style="width: {(100 * snap.done) / snap.toRepeat}%"></div></div>
      </div>
    {/if}
    </div>
    <button class="btn round" onclick={() => (help = true)} title="All keyboard shortcuts (?)" aria-label="Keyboard shortcuts">
      <span class="q">?</span>
    </button>
    <button class="btn round" onclick={() => (settingsOpen = true)} title="Settings ({capText(keyOf('settings'))})" aria-label="Settings">
      <svg width="19" height="19" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4z" fill="none" stroke="currentColor" stroke-width="2"/><path d="M19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.6-2-3.4-2.4 1a7.5 7.5 0 0 0-2.6-1.5L14 2.5h-4l-.4 2.5A7.5 7.5 0 0 0 7 6.5l-2.4-1-2 3.4 2 1.6a7.6 7.6 0 0 0 0 3l-2 1.6 2 3.4 2.4-1c.8.6 1.6 1.1 2.6 1.5l.4 2.5h4l.4-2.5c1-.4 1.8-.9 2.6-1.5l2.4 1 2-3.4-2-1.6z" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/></svg>
    </button>
  </header>

  <main class:solo={!lesson && !loading} data-tauri-drag-region>
    <section class="doc" class:bare={!lesson}>
      {#if lesson}
        <PdfView {lesson} {snap} {word} {zoom} onGoto={gotoPhrase} onMark={(i) => api.mark([i])} />
      {:else if started && lessons.length === 0}
        <Welcome {settings} {micLevel} onChange={changeSettings} onAdd={addPaths} onFolder={addFolder} {dropped} />
      {/if}
      {#if loading}
        <div class="loading" role="status">
          <div class="card load">
            <div class="spinner" aria-hidden="true"></div>
            <div>{loading.stage}…</div>
            {#if loading.progress > 0}
              <div class="bar"><div style="width: {loading.progress * 100}%; background: var(--accent)"></div></div>
              <div class="muted small">Only the first time you open it.</div>
            {/if}
          </div>
        </div>
      {/if}
    </section>
    {#if lesson && snap}
      <PracticePanel {snap} {phrase} {level} {voice} onAct={act} onFilter={(f) => api.cmd({ setFilter: f })} {phraseMarked} {canMark} onExport={exportAnswers} />
    {:else if lesson || loading}
      <aside class="panel-wait card" aria-hidden="true"><div></div><div></div><div></div></aside>
    {/if}
  </main>


  {#if error || notice}
    <div class="toast" class:err={!!error} role="alert">
      <span>{error ?? notice}</span>
      <button class="btn round flat" onclick={() => { error = null; notice = null; }} aria-label="Dismiss">✕</button>
    </div>
  {/if}

  {#if drawer}
    <LessonDrawer
      {lessons}
      {currentId}
      onOpen={openLesson}
      onClose={() => (drawer = false)}
      onAddFiles={addFiles}
      onAddFolder={addFolder}
      onChanged={refresh}
      onError={(m) => (error = m)}
      onReset={(l) => confirmReset(l)}
      onRescan={() => rescan(true)}
    />
  {/if}
  {#if help}
    <Help onClose={() => (help = false)} />
  {/if}
  {#if addOpen}
    <div class="backdrop" onclick={() => (addOpen = false)} role="presentation"></div>
    <div class="sheet card" role="dialog" aria-modal="true" aria-label="Add lessons">
      <div class="sheet-head">
        <h2>Add lessons</h2>
        <button class="btn round" onclick={() => (addOpen = false)} aria-label="Close">✕</button>
      </div>
      <AddLesson onAdd={(p) => { addOpen = false; addPaths(p); }} onFolder={() => { addOpen = false; addFolder(); }} {dropped} />
    </div>
  {/if}
  {#if settingsOpen && settings}
    <Settings {settings} {micLevel} onChange={changeSettings} onClose={() => (settingsOpen = false)} onCleared={refresh} onReset={afterReset} />
  {/if}
  {#if dropping}
    <div class="drop-hint"><div>Drop a folder or a .zip of lessons<span class="muted">, or a recording with its PDF</span></div></div>
  {/if}
  {#if importing}
    <div class="importing card" role="status"><span class="spinner small" aria-hidden="true"></span>{importing}</div>
  {/if}
</div>

<style>
  .app {
    display: grid;
    grid-template-rows: auto 1fr;
    /* columns may shrink below their content: nothing pushes the window wider than it is */
    grid-template-columns: minmax(0, 1fr);
    height: 100%;
    gap: 14px;
    padding: 14px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px 8px 8px;
    min-height: 62px;
    border-radius: var(--r-lg);
    box-shadow: var(--raised);
  }
  /* room for the traffic lights (window buttons) */
  header.mac-frame {
    padding-left: 84px;
  }
  .lessons-btn {
    padding: 0 16px;
  }
  .info {
    display: contents;
  }
  .lessons-btn.over {
    position: relative;
    z-index: 22;
  }
  .q {
    font-weight: 800;
    font-size: 1.05em;
  }
  .title {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 1.05em;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 190px;
  }
  .progress .bar {
    flex: 1;
  }
  .count {
    font-variant-numeric: tabular-nums;
    font-weight: 700;
  }
  main {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 360px;
    gap: 14px;
    min-height: 0;
  }
  .doc {
    position: relative;
    min-width: 0;
    min-height: 0;
    border-radius: var(--r-xl);
    box-shadow: var(--pressed);
    overflow: hidden;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, var(--bg) 55%, transparent);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    z-index: 40;
  }
  .sheet {
    position: fixed;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    width: min(600px, calc(100% - 32px));
    max-height: calc(100% - 40px);
    overflow: auto;
    padding: 22px 26px 26px;
    display: grid;
    gap: 18px;
    z-index: 41;
  }
  .sheet-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .sheet-head h2 {
    margin: 0;
    font-size: 1.3em;
  }
  /* the panel's place while a lesson opens: same size, soft shapes, so nothing jumps */
  .panel-wait {
    display: grid;
    align-content: start;
    gap: 16px;
    padding: 22px;
  }
  .panel-wait div {
    height: 46px;
    border-radius: var(--r-md);
    box-shadow: var(--pressed-sm);
  }
  .panel-wait div:nth-child(2) {
    height: 110px;
  }
  .panel-wait div:nth-child(3) {
    height: 58px;
  }
  main.solo {
    grid-template-columns: 1fr;
  }
  .doc.bare {
    box-shadow: none;
    overflow: visible;
  }
  .loading {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
  }
  .load {
    display: grid;
    justify-items: center;
    gap: 12px;
    width: min(340px, 85%);
    padding: 24px;
    text-align: center;
  }
  .load .bar {
    width: 100%;
  }
  .load .bar > div {
    background: linear-gradient(90deg, var(--accent-a), var(--accent-b));
  }
  .spinner {
    width: 34px;
    height: 34px;
    border-radius: 50%;
    box-shadow: var(--pressed-sm);
    background: conic-gradient(from 0deg, transparent 0 70%, var(--accent) 100%);
    -webkit-mask: radial-gradient(circle, transparent 54%, #000 56%);
    mask: radial-gradient(circle, transparent 54%, #000 56%);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .small {
    font-size: 0.85em;
  }
  .toast {
    position: fixed;
    left: 50%;
    top: 88px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: min(560px, calc(100% - 32px));
    padding: 6px 6px 6px 20px;
    border-radius: var(--r-lg);
    background: var(--bg);
    box-shadow: var(--raised);
    z-index: 30;
  }
  .toast.err {
    color: var(--danger);
  }
  .drop-hint {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 70%, transparent);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    z-index: 50;
    pointer-events: none;
    font-size: 1.15em;
    font-weight: 650;
  }
  .importing {
    position: fixed;
    left: 50%;
    bottom: 24px;
    transform: translateX(-50%);
    z-index: 40;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 20px;
    border-radius: 999px;
    box-shadow: var(--raised);
    font-weight: 600;
    white-space: nowrap;
  }
  .spinner.small {
    width: 16px;
    height: 16px;
    border-width: 2px;
  }
  .drop-hint div {
    padding: 34px 44px;
    border-radius: var(--r-xl);
    box-shadow: var(--pressed), 0 0 0 3px var(--accent-glow);
  }
  @media (max-width: 899px) {
    .app {
      gap: 10px;
      padding: 10px;
    }
    main {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(0, 1fr) auto;
      gap: 10px;
    }
    .progress {
      width: 120px;
    }
  }
  @media (max-width: 560px) {
    .hide-narrow {
      display: none;
    }
    header {
      flex-wrap: wrap;
      gap: 8px;
    }
    .lessons-btn {
      margin-right: auto;
    }
    .info {
      order: 2;
      flex: 1 0 100%;
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 2px 8px 4px;
    }
    .info .progress {
      flex: 1;
      width: auto;
    }
    .info .title {
      flex: 0 1 auto;
    }
    /* the second row doesn't need the room the window buttons take on the first */
    header.mac-frame .info {
      margin-left: -76px;
    }
  }
</style>
