<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { audioExt } from "../lib/api";

  // Two slots, audio and PDF. Choosing (or dropping) both files at once fills both.
  let { onAdd, onFolder, dropped = null }: {
    onAdd: (paths: string[]) => void;
    onFolder: () => void;
    dropped?: string[] | null;
  } = $props();

  let audio = $state<string | null>(null);
  let pdf = $state<string | null>(null);
  let hint = $state<string | null>(null);

  const ext = (p: string) => p.split(".").pop()?.toLowerCase() ?? "";
  const name = (p: string) => p.split(/[\\/]/).pop() ?? p;

  function take(paths: string[]) {
    hint = null;
    for (const p of paths) {
      if (ext(p) === "pdf") pdf = p;
      else if (audioExt.includes(ext(p))) audio = p;
      else hint = `"${name(p)}" isn't an audio file or a PDF.`;
    }
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

  function add() {
    if (!audio) return choose("audio");
    if (!pdf) return choose("pdf");
    {
      onAdd([audio, pdf]);
      audio = null;
      pdf = null;
    }
  }
</script>

<div class="add">
  <div class="slots">
    <button class="slot" class:filled={!!audio} onclick={() => choose("audio")}>
      <span class="ic audio" aria-hidden="true">
        <svg viewBox="0 0 48 48" width="34" height="34">
          {#each [10, 18, 28, 16, 32, 22, 12] as h, i (i)}
            <rect x={6 + i * 5.6} y={24 - h / 2} width="3.2" height={h} rx="1.6" fill="currentColor" />
          {/each}
        </svg>
      </span>
      <b>Audio recording</b>
      <span class="file">{audio ? name(audio) : "mp3 · m4a · wav · flac"}</span>
      <span class="act">{audio ? "Change" : "Choose…"}</span>
    </button>
    <span class="plus" aria-hidden="true">+</span>
    <button class="slot" class:filled={!!pdf} onclick={() => choose("pdf")}>
      <span class="ic pdf" aria-hidden="true">
        <svg viewBox="0 0 48 48" width="34" height="34">
          <path d="M13 6h15l9 9v25a3 3 0 0 1-3 3H13a3 3 0 0 1-3-3V9a3 3 0 0 1 3-3z" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linejoin="round" />
          <path d="M28 6v9h9M16 24h16M16 30h16M16 36h10" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </span>
      <b>Its PDF</b>
      <span class="file">{pdf ? name(pdf) : "the text the teacher reads"}</span>
      <span class="act">{pdf ? "Change" : "Choose…"}</span>
    </button>
  </div>
  <p class="tip muted">Tip: select <b>both files at once</b> in either window, or drop them here together.</p>
  {#if hint}<p class="warn">{hint}</p>{/if}
  <div class="row">
    <button class="btn" class:primary={!!audio && !!pdf} onclick={add}>
      {audio && pdf ? "Add lesson" : !audio && !pdf ? "Choose the files…" : !audio ? "Now choose the audio…" : "Now choose the PDF…"}
    </button>
    <span class="or muted">or</span>
    <button class="btn" onclick={onFolder}>Add a whole folder…</button>
  </div>
</div>

<style>
  .add {
    display: grid;
    gap: 14px;
  }
  .slots {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 12px;
  }
  .slot {
    display: grid;
    justify-items: center;
    gap: 4px;
    min-width: 0;
    padding: 18px 14px 14px;
    border: 0;
    border-radius: var(--r-lg);
    background: var(--bg);
    box-shadow: var(--pressed);
    transition: box-shadow 0.2s, transform 0.15s;
  }
  .slot:hover {
    box-shadow: var(--raised-sm);
    transform: translateY(-2px);
  }
  .slot:hover .act {
    text-decoration: underline;
  }
  .slot.filled {
    box-shadow: var(--raised-sm);
  }
  .ic {
    display: grid;
    place-items: center;
    width: 60px;
    height: 60px;
    margin-bottom: 6px;
    border-radius: 20px;
    box-shadow: var(--raised-xs);
  }
  .ic.audio {
    color: var(--teacher);
  }
  .ic.pdf {
    color: var(--accent);
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
    margin-top: 4px;
    font-size: 0.82em;
    font-weight: 650;
    color: var(--accent);
  }
  .plus {
    font-size: 1.6em;
    color: var(--muted);
  }
  .tip,
  .warn {
    margin: 0;
    font-size: 0.86em;
    text-align: center;
  }
  .warn {
    color: var(--danger);
  }
  .row {
    display: flex;
    justify-content: center;
    align-items: center;
    flex-wrap: wrap;
    gap: 12px;
  }
  @media (max-width: 520px) {
    .slots {
      grid-template-columns: 1fr;
    }
    .plus {
      text-align: center;
    }
  }
</style>
