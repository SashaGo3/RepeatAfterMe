<script lang="ts">
  import { scrollHints } from "../lib/scroll";
  import type { Filter, PhraseView, Snapshot } from "../lib/api";
  import { capText, keyOf, spaceLabel, type Action } from "../lib/keys";

  let { snap, phrase, level, voice, onAct, onFilter, phraseMarked = false, canMark = true, onExport }: {
    snap: Snapshot;
    phrase: PhraseView | null;
    level: number;
    voice: boolean;
    onAct: (a: Action) => void;
    onFilter: (f: Filter) => void;
    phraseMarked?: boolean;
    canMark?: boolean;
    onExport: () => void;
  } = $props();

  const status = $derived.by(() => {
    switch (snap.mode) {
      case "idle": return { text: snap.listenOnly ? "Listen-only mode" : "Ready", tone: "" };
      case "teacher": return { text: snap.listenOnly ? "Listening" : "Listen…", tone: "teacher" };
      case "turn": return { text: "Your turn: say it…", tone: "you" };
      case "compare": return { text: "Teacher, then you", tone: "teacher" };
      case "mine": return { text: "Your take", tone: "you" };
      case "decide": return { text: snap.hasTake ? "Your call: good, or again?" : "Your call", tone: "" };
    }
  });

  const bars = 18;
  // "heard" holds for 0.4 s, so tiny gaps between words don't make the light blink
  let heard = $state(false);
  let hold: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    if (voice) {
      clearTimeout(hold);
      heard = true;
    } else if (heard) {
      clearTimeout(hold);
      hold = setTimeout(() => (heard = false), 400);
    }
  });
  const cap = (a: Action) => capText(keyOf(a));
  const deciding = $derived(snap.mode === "decide" || snap.mode === "compare" || snap.mode === "mine");
  const filters: [Filter, string][] = [["all", "All"], ["notDone", "To do"], ["marked", "Marked"]];
  const happy = $derived(!!snap.note && (snap.note.startsWith("Lesson done") || snap.note.startsWith("End")));
</script>

<aside class="panel" aria-live="polite" use:scrollHints>
  <!-- every slot is always there (buttons only turn on and off, notes show in the status line), so nothing moves -->
  <div class="where">
    <span>Phrase {snap.phrase + 1} <span class="muted">of {snap.phrases}</span></span>
    {#if snap.tries > 0}<span class="muted">· {snap.tries} {snap.tries === 1 ? "try" : "tries"}</span>{/if}
    <span class="chips">
      {#if phraseMarked}<span class="chip marked" title="You marked words in this phrase">● marked</span>{/if}
      {#if snap.phraseDone}<span class="chip done">✓ good</span>{/if}
    </span>
  </div>

  <p class="text" class:explain={phrase && !phrase.inPdf}>{phrase?.text ?? ""}</p>

  <div class="status {snap.note ? (happy ? 'happy' : 'warn') : status.tone}" class:heard={snap.mode === "turn" && heard}>
    <span class="dot"></span>
    <span class="status-text">{snap.note ?? status.text}</span>
    {#if snap.mode === "turn"}
      <span class="meter" aria-hidden="true">
        {#each Array(bars) as _, i (i)}
          <i class:on={i < Math.round(level * bars)}></i>
        {/each}
      </span>
    {/if}
  </div>

  <div class="main">
    <button class="btn big wide" class:primary={!deciding} onclick={() => onAct("space")}>
      {spaceLabel(snap)} <span class="kbd">{cap("space")}</span>
    </button>
    <button class="btn big" class:good={deciding} disabled={!deciding} onclick={() => onAct("good")} title="Good: next phrase">Good <span class="kbd">{cap("good")}</span></button>
    <button class="btn big" disabled={!deciding || snap.listenOnly} onclick={() => onAct("again")}>Again <span class="kbd">{cap("again")}</span></button>
  </div>

  <div class="grid">
    <button class="btn" onclick={() => onAct("prev")} title="Previous phrase (B, ← or Esc)">Back <span class="kbd">{cap("prev")}</span></button>
    <button class="btn" onclick={() => onAct("next")} title="Next phrase (N or →)">Next <span class="kbd">{cap("next")}</span></button>
    <button class="btn" onclick={() => onAct("teacher")} disabled={snap.mode === "turn"}>Teacher <span class="kbd">{cap("teacher")}</span></button>
    <button class="btn" onclick={() => onAct("mine")} disabled={!snap.hasTake || snap.mode === "turn"}>My take <span class="kbd">{cap("mine")}</span></button>
    <button class="btn" class:on={phraseMarked} disabled={!canMark} onclick={() => onAct("mark")} title={canMark ? "Mark every printed word of this phrase (right-click a word to mark just that word)" : "Nothing printed to mark in this phrase"}>{phraseMarked ? "Unmark" : "Mark"} <span class="kbd">{cap("mark")}</span></button>
    <button class="btn" onclick={() => onAct("stop")} disabled={snap.mode === "idle"}>Stop <span class="kbd">{cap("stop")}</span></button>
  </div>

  <div class="modes">
    <label class="mode">
      <input type="checkbox" checked={snap.listenOnly} onchange={() => onAct("listenOnly")} tabindex="-1" />
      <span class="mode-name">Listen only <span class="muted">· no turns</span></span>
      <span class="kbd">{cap("listenOnly")}</span>
      <span class="switch" class:on={snap.listenOnly}></span>
    </label>
    <div class="mode" title="Which phrases Back, Next and Good go through">
      <span class="mode-name">Phrases</span>
      <span class="kbd">{cap("filter")}</span>
      <span class="pick" role="radiogroup" aria-label="Which phrases">
        {#each filters as [f, label] (f)}
          <button role="radio" aria-checked={snap.filter === f} class:on={snap.filter === f} onclick={() => onFilter(f)} tabindex="-1">{label}</button>
        {/each}
      </span>
    </div>
  </div>

  <div class="links">
    <button class="btn small" onclick={() => onAct("reset")} title="Every phrase back to not done (your marks stay)">
      <svg width="15" height="15" viewBox="0 0 24 24" aria-hidden="true"><path d="M3 12a9 9 0 1 0 3-6.7M3 4v5h5" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
      Start over
    </button>
    <button class="btn small" onclick={onExport} title="One audio file: the teacher, then you, for every phrase you practised">
      <svg width="15" height="15" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4v11m0 0l-4.5-4.5M12 15l4.5-4.5M5 20h14" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
      Export answers
    </button>
  </div>
</aside>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 20px;
    border-radius: var(--r-xl);
    box-shadow: var(--raised);
    overflow: auto;
  }
  .panel > * {
    flex-shrink: 0;
  }
  .where {
    display: flex;
    gap: 6px;
    align-items: center;
    min-height: 26px;
    font-size: 0.88em;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .where > span:first-child {
    color: var(--text);
    font-weight: 650;
  }
  .chips {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }
  .chip {
    padding: 3px 10px;
    border-radius: 999px;
    box-shadow: var(--pressed-sm);
    font-weight: 700;
    font-size: 0.92em;
    white-space: nowrap;
  }
  .chip.marked {
    color: var(--marked);
  }
  .chip.done {
    color: var(--you);
  }
  .text {
    margin: 0;
    padding: 14px 18px;
    border-radius: var(--r-lg);
    box-shadow: var(--pressed);
    font-size: 1.2em;
    line-height: 1.38;
    font-weight: 650;
    /* a fixed box: three lines show, a longer phrase scrolls inside it */
    box-sizing: border-box;
    height: calc(3 * 1.38em + 28px);
    overflow-y: auto;
    user-select: text;
    -webkit-user-select: text;
    text-wrap: pretty;
  }
  .text.explain {
    font-weight: 450;
    color: var(--muted);
    font-style: italic;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 16px;
    border-radius: 999px;
    box-shadow: var(--raised-xs);
    font-weight: 650;
  }
  .status-text {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--muted) 40%, transparent);
    flex: none;
    transition: transform 0.15s, box-shadow 0.15s;
  }
  .status.teacher .dot {
    background: var(--teacher);
    box-shadow: 0 0 10px var(--teacher);
    animation: pulse 1s ease-in-out infinite;
  }
  .status.you .dot {
    background: var(--you);
    box-shadow: 0 0 10px var(--you);
    animation: pulse 0.8s ease-in-out infinite;
  }
  .status.heard .dot {
    animation: none;
    transform: scale(1.25);
    box-shadow: 0 0 14px var(--you);
  }
  .status.teacher {
    color: var(--teacher);
  }
  .status.you,
  .status.happy {
    color: var(--you);
  }
  .status.happy .dot {
    background: var(--you);
  }
  .status.warn {
    color: var(--danger);
  }
  .status.warn .dot {
    background: var(--danger);
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .meter {
    display: inline-flex;
    gap: 3px;
    align-items: center;
    height: 16px;
    margin-left: auto;
  }
  .meter i {
    width: 4px;
    height: 100%;
    border-radius: 2px;
    background: color-mix(in srgb, var(--muted) 22%, transparent);
  }
  .meter i.on {
    background: var(--you);
  }
  .main {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 12px;
  }
  .big {
    min-height: 54px;
    font-size: 1.05em;
    border-radius: var(--r-lg);
    white-space: nowrap;
  }
  .wide {
    grid-column: 1 / -1;
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 10px;
  }
  .big,
  .grid .btn {
    min-width: 0;
    overflow: hidden;
  }
  .grid .btn {
    min-height: 42px;
    padding: 0 12px;
    font-size: 0.92em;
    white-space: nowrap;
  }
  .modes {
    display: grid;
    border-radius: var(--r-md);
    box-shadow: var(--pressed-sm);
    padding: 4px 14px;
  }
  .mode {
    display: flex;
    gap: 10px;
    align-items: center;
    min-height: 44px;
    font-size: 0.92em;
    cursor: pointer;
  }
  .mode + .mode {
    border-top: 1px solid color-mix(in srgb, var(--muted) 18%, transparent);
  }
  .mode-name {
    flex: 1;
    white-space: nowrap;
  }
  .pick {
    flex: none;
    display: inline-flex;
    padding: 3px;
    gap: 2px;
    border-radius: 999px;
    box-shadow: var(--pressed-sm);
  }
  .pick button {
    white-space: nowrap;
    border: 0;
    background: none;
    padding: 4px 10px;
    border-radius: 999px;
    color: var(--muted);
    font: inherit;
    font-size: 0.92em;
    font-weight: 600;
    cursor: pointer;
  }
  .pick button.on {
    background: var(--bg);
    box-shadow: var(--raised-xs);
    color: var(--accent);
  }
  .mode input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  /* lesson-level actions: quieter than the practice buttons, at the bottom of the panel */
  .links {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 10px;
    margin-top: auto;
  }
  .small {
    min-height: 38px;
    padding: 0 10px;
    gap: 0.5em;
    border-radius: var(--r-sm);
    box-shadow: var(--raised-xs);
    color: var(--muted);
    font-size: 0.86em;
    font-weight: 600;
    white-space: nowrap;
  }
  .small svg {
    flex: none;
    opacity: 0.85;
  }
  @media (max-width: 899px) {
    .panel {
      padding: 14px;
      gap: 10px;
    }
    .text {
      font-size: 1.08em;
      padding: 12px 14px;
    }
    .modes,
    .links {
      display: none;
    }
    .big {
      min-height: 46px;
    }
    .main,
    .grid {
      gap: 8px;
    }
    .grid .btn {
      min-height: 38px;
    }
  }
</style>
