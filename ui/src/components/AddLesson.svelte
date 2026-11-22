<script lang="ts">
  import { onMount, tick } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { audioExt } from "../lib/api";
  import { keyLayer } from "../lib/keys";

  // Three ways in: a folder, a .zip file, or one recording and then its PDF. Dropping works for all three
  // (the window takes the drop; loose files arrive here in `dropped`).
  let { onAdd, onFolder, dropped = null }: {
    onAdd: (paths: string[]) => void;
    onFolder: () => void;
    dropped?: string[] | null;
  } = $props();

  let audio = $state<string | null>(null);
  let pdf = $state<string | null>(null);
  let hint = $state<string | null>(null);
  /** the audio + PDF slots are open */
  let pairing = $state(false);
  let addBtn = $state<HTMLButtonElement | null>(null);

  const ext = (p: string) => p.split(".").pop()?.toLowerCase() ?? "";
  const name = (p: string) => p.split(/[\\/]/).pop() ?? p;

  function take(paths: string[]) {
    hint = null;
    for (const p of paths) {
      if (ext(p) === "pdf") pdf = p;
      else if (audioExt.includes(ext(p))) audio = p;
      else hint = `"${name(p)}" is not an audio file or a PDF.`;
    }
    if (audio || pdf) pairing = true;
  }

  $effect(() => {
    if (dropped?.length) take(dropped);
  });

  async function choose(kind: "audio" | "pdf") {
    const picked = await openDialog({
      multiple: true,
      title: kind === "audio" ? "Choose the lesson's audio recording (you can pick its PDF too)" : "Choose the lesson's PDF (you can pick its audio too)",
      filters: [
        kind === "audio"
          ? { name: "Audio recording (and PDF)", extensions: [...audioExt, "pdf"] }
          : { name: "PDF (and audio recording)", extensions: ["pdf", ...audioExt] },
      ],
    });
    if (picked) take(Array.isArray(picked) ? picked : [picked]);
  }

  // 3: the audio first, then its PDF
  async function pair() {
    pairing = true;
    if (!audio) await choose("audio");
    if (audio && !pdf) await choose("pdf");
    await tick();
    if (audio && pdf) addBtn?.focus();
  }

  async function chooseZip() {
    const picked = await openDialog({
      multiple: false,
      title: "Choose a .zip file of lessons",
      filters: [{ name: "Zip file", extensions: ["zip"] }],
    });
    if (picked) onAdd([picked as string]);
  }

  function add() {
    if (!audio || !pdf) return pair();
    onAdd([audio, pdf]);
    audio = null;
    pdf = null;
    pairing = false;
  }

  onMount(() =>
    keyLayer((e) => {
      if (e.repeat) return false;
      switch (e.code) {
        case "Digit1": case "Numpad1": onFolder(); return true;
        case "Digit2": case "Numpad2": chooseZip(); return true;
        case "Digit3": case "Numpad3": pair(); return true;
      }
      // Enter adds the lesson when both files are there (unless another button has the focus)
      if (e.key === "Enter" && audio && pdf && (e.target as HTMLElement)?.tagName !== "BUTTON") {
        add();
        return true;
      }
      return false;
    }),
  );
</script>

<div class="add">
  <div class="drop">
    <span class="drop-ic" aria-hidden="true">
      <svg viewBox="0 0 24 24" width="22" height="22"><path d="M12 3v11M7.5 9.5 12 14l4.5-4.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /><path d="M4 15v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg>
    </span>
    <b>Drag and drop your lessons here</b>
    <span class="muted">A folder, a .zip file, or a recording with its PDF</span>
  </div>

  <div class="or muted"><span>or choose</span></div>

  <div class="ways">
    <button class="way" onclick={onFolder}>
      <span class="kbd n">1</span>
      <span class="ic folder" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="26" height="26"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" /></svg>
      </span>
      <b>A folder</b>
      <span class="sub">One lesson or many</span>
    </button>
    <button class="way" onclick={chooseZip}>
      <span class="kbd n">2</span>
      <span class="ic zip" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="26" height="26"><path d="M6 3h8l4 4v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" /><path d="M10 3v2h2v2h-2v2h2v2h-2" fill="none" stroke="currentColor" stroke-width="1.8" /><rect x="9.5" y="13" width="3" height="4" rx="1" fill="none" stroke="currentColor" stroke-width="1.8" /></svg>
      </span>
      <b>A .zip file</b>
      <span class="sub">A folder in one file</span>
    </button>
    <button class="way" class:on={pairing} onclick={pair}>
      <span class="kbd n">3</span>
      <span class="ic pair" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="26" height="26">
          {#each [4, 8, 12, 7, 5] as h, i (i)}<rect x={2 + i * 2.4} y={12 - h / 2} width="1.5" height={h} rx="0.75" fill="currentColor" />{/each}
          <path d="M15 5h4l2 2v11a1 1 0 0 1-1 1h-5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" />
        </svg>
      </span>
      <b>Audio, then PDF</b>
      <span class="sub">One lesson</span>
    </button>
  </div>

  {#if pairing}
    <div class="slots">
      <button class="slot" class:filled={!!audio} onclick={() => choose("audio")}>
        <b>Audio recording</b>
        <span class="file">{audio ? name(audio) : "mp3 · m4a · wav · flac"}</span>
        <span class="act">{audio ? "Change" : "Choose…"}</span>
      </button>
      <span class="plus" aria-hidden="true">+</span>
      <button class="slot" class:filled={!!pdf} onclick={() => choose("pdf")}>
        <b>Its PDF</b>
        <span class="file">{pdf ? name(pdf) : "the text the teacher reads"}</span>
        <span class="act">{pdf ? "Change" : "Choose…"}</span>
      </button>
    </div>
    <div class="row">
      <button class="btn" class:primary={!!audio && !!pdf} onclick={add} bind:this={addBtn}>
        {audio && pdf ? "Add lesson" : !audio ? "Choose the audio…" : "Now choose the PDF…"}
        {#if audio && pdf}<span class="kbd">↵</span>{/if}
      </button>
    </div>
  {/if}
  {#if hint}<p class="warn">{hint}</p>{/if}
</div>

<style>
  .add {
    display: grid;
    gap: 14px;
  }
  .drop {
    display: grid;
    justify-items: center;
    gap: 4px;
    padding: 18px 16px;
    border: 2px dashed color-mix(in srgb, var(--muted) 35%, transparent);
    border-radius: var(--r-lg);
    text-align: center;
  }
  .drop .muted {
    font-size: 0.86em;
  }
  .drop-ic {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    margin-bottom: 4px;
    border-radius: 50%;
    box-shadow: var(--raised-xs);
    color: var(--accent);
  }
  .or {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 0.82em;
  }
  .or::before,
  .or::after {
    content: "";
    flex: 1;
    height: 1px;
    background: color-mix(in srgb, var(--muted) 25%, transparent);
  }
  .ways {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }
  .way {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 3px;
    min-width: 0;
    padding: 16px 10px 14px;
    border: 0;
    border-radius: var(--r-lg);
    background: var(--bg);
    box-shadow: var(--raised-sm);
    transition: box-shadow 0.2s, transform 0.15s;
  }
  .way:hover {
    transform: translateY(-2px);
  }
  .way.on {
    box-shadow: var(--pressed);
  }
  .way .n {
    position: absolute;
    top: 8px;
    left: 8px;
  }
  .ic {
    display: grid;
    place-items: center;
    width: 50px;
    height: 50px;
    margin-bottom: 6px;
    border-radius: 16px;
    box-shadow: var(--raised-xs);
  }
  .ic.folder {
    color: var(--accent);
  }
  .ic.zip {
    color: var(--muted);
  }
  .ic.pair {
    color: var(--teacher);
  }
  .sub {
    font-size: 0.82em;
    color: var(--muted);
  }
  .slots {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 12px;
    animation: fade 0.2s ease;
  }
  @keyframes fade {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  .slot {
    display: grid;
    justify-items: center;
    gap: 3px;
    min-width: 0;
    padding: 12px;
    border: 0;
    border-radius: var(--r-lg);
    background: var(--bg);
    box-shadow: var(--pressed);
    transition: box-shadow 0.2s;
  }
  .slot:hover .act {
    text-decoration: underline;
  }
  .slot.filled {
    box-shadow: var(--raised-sm);
  }
  .file {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85em;
    color: var(--muted);
  }
  .filled .file {
    color: var(--text);
    font-weight: 600;
  }
  .act {
    font-size: 0.82em;
    font-weight: 650;
    color: var(--accent);
  }
  .plus {
    font-size: 1.4em;
    color: var(--muted);
  }
  .warn {
    margin: 0;
    font-size: 0.86em;
    text-align: center;
    color: var(--danger);
  }
  .row {
    display: flex;
    justify-content: center;
  }
  .row .btn {
    gap: 10px;
  }
  @media (max-width: 520px) {
    .ways,
    .slots {
      grid-template-columns: 1fr;
    }
    .plus {
      text-align: center;
    }
  }
</style>
