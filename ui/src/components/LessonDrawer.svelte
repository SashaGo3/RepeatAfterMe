<script lang="ts">
  import { scrollHints } from "../lib/scroll";
  import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
  import { api, type LessonInfo } from "../lib/api";
  import { capText, keyOf } from "../lib/keys";

  let { lessons, currentId, onOpen, onClose, onAddFiles, onAddFolder, onChanged, onError, onReset, onRescan }: {
    lessons: LessonInfo[];
    currentId: string | null;
    onOpen: (id: string) => void;
    onClose: () => void;
    onAddFiles: () => void;
    onAddFolder: () => void;
    onChanged: () => void;
    onError: (m: string) => void;
    onReset: (l: { id: string; name: string }) => void;
    onRescan: () => void;
  } = $props();

  type Sort = "folders" | "recent" | "time" | "name";
  const SORT_KEY = "ram.lessonSort";
  let sort = $state<Sort>((() => { try { return (localStorage.getItem(SORT_KEY) as Sort) || "folders"; } catch { return "folders"; } })());
  $effect(() => { try { localStorage.setItem(SORT_KEY, sort); } catch {} });
  let query = $state("");
  let menuFor = $state<string | null>(null); // a lesson's audio path, or "group:<name>"
  // the open menu sits over everything, next to its ⋯ button (below it, or above when there's no room)
  let menuAt = $state<{ right: number; top?: number; bottom?: number }>({ right: 0 });

  function toggleMenu(key: string, e: MouseEvent) {
    if (menuFor === key) {
      menuFor = null;
      return;
    }
    const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const right = Math.max(8, window.innerWidth - b.right);
    menuAt = window.innerHeight - b.bottom < 260 ? { right, bottom: window.innerHeight - b.top + 4 } : { right, top: b.bottom + 4 };
    menuFor = key;
  }

  const menuLesson = $derived(menuFor && !menuFor.startsWith("group:") ? lessons.find((l) => l.audio === menuFor) : undefined);
  const menuGroup = $derived(menuFor?.startsWith("group:") ? menuFor.slice(6) : null);
  let renaming = $state<string | null>(null);
  let newName = $state("");

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    let v = q ? lessons.filter((l) => l.name.toLowerCase().includes(q) || l.group.toLowerCase().includes(q)) : lessons;
    if (sort === "recent") v = [...v].sort((a, b) => b.last - a.last);
    else if (sort === "time") v = [...v].sort((a, b) => b.secs - a.secs);
    else if (sort === "name") v = [...v].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
    return v;
  });

  // in folder order: one section per folder; sorted any other way: one list
  const groups = $derived.by(() => {
    if (sort !== "folders") return [["", shown]] as [string, LessonInfo[]][];
    const m = new Map<string, LessonInfo[]>();
    for (const l of shown) {
      if (!m.has(l.group)) m.set(l.group, []);
      m.get(l.group)!.push(l);
    }
    return [...m.entries()];
  });

  const totalSecs = $derived(lessons.reduce((t, l) => t + l.secs, 0));

  /** 0 → "", 40 s → "<1 min", 754 s → "12 min", 4000 s → "1 h 06 min" */
  function time(secs: number): string {
    if (secs <= 0) return "";
    const m = Math.round(secs / 60);
    if (m < 1) return "<1 min";
    if (m < 60) return `${m} min`;
    return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min`;
  }

  /** when, in words: "today", "yesterday", "3 days ago", "12 Mar" */
  function ago(unix: number): string {
    if (!unix) return "";
    const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    const days = Math.round((day(new Date()) - day(new Date(unix * 1000))) / 86400000);
    if (days <= 0) return "today";
    if (days === 1) return "yesterday";
    if (days < 7) return `${days} days ago`;
    return new Date(unix * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }

  function details(l: LessonInfo): string {
    const parts = [time(l.secs), ago(l.last)].filter(Boolean);
    if (sort !== "folders" && l.group) parts.unshift(l.group);
    return parts.length ? parts.join(" · ") : "not practised yet";
  }

  async function changePdf(l: LessonInfo) {
    menuFor = null;
    const pdf = await openDialog({ title: `PDF for "${l.name}"`, filters: [{ name: "PDF", extensions: ["pdf"] }] });
    if (!pdf) return;
    try {
      await api.setPdf(l.audio, pdf as string);
      onChanged();
      if (l.id === currentId) onOpen(l.id);
    } catch (e) {
      onError(String(e));
    }
  }

  async function remove(l: LessonInfo) {
    menuFor = null;
    await api.remove(l.audio).catch((e) => onError(String(e)));
    onChanged();
  }

  async function clearMarks(l: LessonInfo) {
    menuFor = null;
    await api.clearMarks(l.id).catch((e) => onError(String(e)));
  }

  async function removeGroup(group: string, n: number) {
    menuFor = null;
    const yes = await ask(`The ${n} lesson${n === 1 ? "" : "s"} in "${group}" leave the list. The files stay where they are, and your progress is kept if you add them again.`, { title: "Remove this folder from the list?", kind: "warning", okLabel: "Remove" });
    if (!yes) return;
    await api.removeGroup(group).catch((e) => onError(String(e)));
    onChanged();
  }

  function startRename(group: string) {
    menuFor = null;
    renaming = group;
    newName = group;
  }

  async function finishRename(save: boolean) {
    const from = renaming;
    renaming = null;
    const to = newName.trim();
    if (!save || !from || !to || to === from) return;
    await api.renameGroup(from, to).catch((e) => onError(String(e)));
    onChanged();
  }

  function focus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  function file(p: string) {
    return p.split(/[\\/]/).pop();
  }

  let box: HTMLDivElement | undefined = $state();
  $effect(() => {
    box?.querySelector<HTMLButtonElement>(".current .open, .open")?.focus();
  });
</script>

<div class="backdrop" onclick={onClose} role="presentation"></div>
<div class="drawer card" bind:this={box} role="dialog" aria-label="Lessons">
  <!-- no header of its own: the window's Lessons button sits on top of this space and closes the list -->
  <div class="add">
    <button class="btn" onclick={onAddFolder} title="Add a folder of lessons">+ Folder <span class="kbd">{capText(keyOf("addFolder"))}</span></button>
    <button class="btn" onclick={onAddFiles} title="Add a recording and its PDF">+ Files <span class="kbd">{capText(keyOf("addFiles"))}</span></button>
    <button class="btn round" onclick={onRescan} title="Look in your folders for new lessons ({capText(keyOf('rescan'))})" aria-label="Look for new lessons">
      <svg width="17" height="17" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
  </div>
  {#if lessons.length > 0}
    <div class="tools">
      <input class="field search" type="search" placeholder="Search" bind:value={query} aria-label="Search lessons" />
      <select class="field sort" bind:value={sort} aria-label="Sort lessons">
        <option value="folders">By folder</option>
        <option value="recent">Recently practised</option>
        <option value="time">Most practised</option>
        <option value="name">By name</option>
      </select>
    </div>
    {#if totalSecs > 0}
      <p class="total muted small">Practised {time(totalSecs)} in all</p>
    {/if}
  {/if}
  <div class="list" use:scrollHints onscroll={() => (menuFor = null)}>
    {#if lessons.length === 0}
      <p class="muted pad">No lessons yet. Add a folder, or drop files on the window.</p>
    {:else if shown.length === 0}
      <p class="muted pad">No lesson matches "{query}".</p>
    {/if}
    {#each groups as [group, items] (group)}
      {#if sort === "folders"}
        <div class="gh" class:open={menuFor === "group:" + group}>
          {#if renaming === group}
            <input
              class="field rename"
              bind:value={newName}
              use:focus
              onkeydown={(e) => { if (e.key === "Enter") finishRename(true); else if (e.key === "Escape") { e.stopPropagation(); finishRename(false); } }}
              onblur={() => finishRename(true)}
              aria-label="Folder name"
            />
          {:else}
            <h3>{group || "Lessons"}</h3>
            {#if items.some((l) => l.secs > 0)}<span class="gtime">{time(items.reduce((t, l) => t + l.secs, 0))}</span>{/if}
            <button class="more" class:active={menuFor === "group:" + group} onclick={(e) => toggleMenu("group:" + group, e)} aria-label="More for {group}">⋯</button>
          {/if}
        </div>
      {/if}
      {#each items as l (l.audio)}
        <div class="item" class:current={l.id === currentId} class:missing={l.missing} class:open={menuFor === l.audio}>
          <button
            class="open"
            onclick={() => (l.missing ? changePdf(l) : onOpen(l.id))}
            title={l.missing ? "A file was moved or deleted" : `${file(l.audio)} + ${file(l.pdf)}`}
          >
            <span class="what">
              <span class="name">{l.name}</span>
              {#if !l.missing}<span class="meta">{details(l)}</span>{/if}
            </span>
            {#if l.missing}
              <span class="warn">Files missing</span>
            {:else if l.total}
              <span class="prog">
                <span class="bar"><span style="width: {(100 * l.done) / l.total}%"></span></span>
                <span class="num">{l.done}/{l.total}</span>
              </span>
            {:else}
              <span class="muted small">new</span>
            {/if}
          </button>
          <button class="more" class:active={menuFor === l.audio} onclick={(e) => toggleMenu(l.audio, e)} aria-label="More for {l.name}">⋯</button>
        </div>
      {/each}
    {/each}
  </div>
</div>

{#if menuFor}
  <!-- a click anywhere else closes the menu -->
  <div class="menu-away" onclick={() => (menuFor = null)} role="presentation"></div>
  <div class="menu" role="menu" style="right: {menuAt.right}px; {menuAt.top !== undefined ? `top: ${menuAt.top}px` : `bottom: ${menuAt.bottom}px`}">
    {#if menuGroup !== null}
      <button onclick={() => startRename(menuGroup)}>Rename…</button>
      <button class="danger" onclick={() => removeGroup(menuGroup, lessons.filter((l) => l.group === menuGroup).length)}>Remove from list…</button>
    {:else if menuLesson}
      {@const l = menuLesson}
      <div class="muted small files">{file(l.audio)}<br />{file(l.pdf)}</div>
      {#if !l.missing}
        <button onclick={() => { menuFor = null; onReset({ id: l.id, name: l.name }); }} disabled={l.done === 0}>Start over…</button>
        <button onclick={() => clearMarks(l)}>Clear marked words</button>
      {/if}
      <button onclick={() => changePdf(l)}>Use a different PDF…</button>
      <button class="danger" onclick={() => remove(l)}>Remove from list</button>
    {/if}
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, var(--bg) 50%, transparent);
    backdrop-filter: blur(5px);
    -webkit-backdrop-filter: blur(5px);
    z-index: 20;
  }
  .drawer {
    position: fixed;
    top: 14px;
    bottom: 14px;
    left: 14px;
    width: min(390px, calc(100vw - 28px));
    display: flex;
    flex-direction: column;
    z-index: 21;
    animation: slide 0.24s cubic-bezier(0.2, 0.9, 0.3, 1.05);
  }
  @keyframes slide {
    from {
      transform: translateX(-24px);
      opacity: 0;
    }
  }
  .add {
    display: flex;
    gap: 12px;
    /* below the header row, where the Lessons (✕) button stays */
    padding: 72px 16px 16px;
  }
  .add .btn {
    flex: 1;
    padding: 0 12px;
    font-size: 0.9em;
    white-space: nowrap;
  }
  .add .btn.round {
    flex: none;
  }
  .tools {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    padding: 0 16px 6px;
  }
  .tools .field {
    min-height: 38px;
    font-size: 0.9em;
  }
  .search {
    box-sizing: border-box;
    width: 100%;
    padding: 0 14px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--bg);
    box-shadow: var(--pressed-sm);
    color: var(--text);
    font: inherit;
  }
  .sort {
    width: auto;
  }
  .total {
    margin: 4px 24px 0;
  }
  /* a folder's heading: name, its time, and its ⋯ menu */
  .gh {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 12px 0 2px;
    min-height: 34px;
  }
  .gh.open {
    z-index: 11;
  }
  .gh h3 {
    flex: 1;
    min-width: 0;
    margin: 0 0 0 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .gtime {
    font-size: 0.78em;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .gh .more {
    height: 30px;
  }
  .rename {
    flex: 1;
    min-height: 34px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-sm);
    background: var(--bg);
    box-shadow: var(--pressed-sm);
    color: var(--text);
    font: inherit;
    font-weight: 650;
  }
  .what {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 2px;
  }
  .meta {
    font-size: 0.78em;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .list {
    overflow: auto;
    padding: 4px 14px 22px;
  }
  h3 {
    margin: 0;
    font-size: 0.78em;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .item {
    position: relative;
    display: flex;
    align-items: stretch;
    margin: 4px 0;
    border-radius: var(--r-md);
    transition: box-shadow 0.18s, transform 0.12s;
  }
  /* the row with its menu open sits above the rows below it */
  .item.open {
    z-index: 10;
  }
  .item:hover {
    box-shadow: var(--raised-sm);
    transform: translateY(-1px);
  }
  .item:hover .name {
    color: var(--accent);
  }
  .item.current {
    box-shadow: var(--pressed-sm);
  }
  .item.current .name {
    color: var(--accent);
    font-weight: 650;
  }
  .open {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    padding: 6px 8px 6px 16px;
    border: 0;
    background: transparent;
    text-align: left;
    min-width: 0;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .missing .name {
    color: var(--muted);
    text-decoration: line-through;
  }
  .warn {
    color: var(--danger);
    font-size: 0.82em;
  }
  .prog {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }
  .prog .bar {
    display: block;
    width: 58px;
  }
  .prog .bar span {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: linear-gradient(90deg, var(--you-a), var(--you-b));
  }
  .num {
    font-size: 0.82em;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    min-width: 3.2em;
    text-align: right;
  }
  .more {
    width: 40px;
    border: 0;
    background: transparent;
    color: var(--muted);
    border-radius: var(--r-sm);
    font-size: 1.1em;
  }
  .more:hover,
  .more.active {
    color: var(--text);
  }
  .menu-away {
    position: fixed;
    inset: 0;
    z-index: 30;
  }
  .menu {
    position: fixed;
    z-index: 31;
    max-width: min(320px, calc(100vw - 16px));
    display: grid;
    min-width: 220px;
    padding: 8px;
    border-radius: var(--r-md);
    background: var(--bg);
    box-shadow: var(--raised);
  }
  .menu button {
    padding: 10px 12px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    text-align: left;
  }
  .menu button:hover:not(:disabled) {
    box-shadow: var(--pressed-sm);
  }
  .menu button:disabled {
    color: var(--muted);
    opacity: 0.6;
  }
  .menu button.danger {
    color: var(--danger);
  }
  .files {
    padding: 4px 10px 8px;
    word-break: break-all;
  }
  .small {
    font-size: 0.82em;
  }
  .pad {
    padding: 12px;
  }
</style>
