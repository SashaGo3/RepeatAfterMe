<script lang="ts">
  import { scrollHints } from "../lib/scroll";
  import { pageUrl, type LessonView, type Snapshot } from "../lib/api";

  let { lesson, snap, word, zoom, onGoto, onMark }: {
    lesson: LessonView;
    snap: Snapshot | null;
    word: number;
    zoom: number;
    onGoto: (phrase: number) => void;
    onMark: (word: number) => void;
  } = $props();
  const marked = $derived(new Set(lesson.marked));

  let scroller: HTMLDivElement;
  let width = $state(800);
  let renderWidth = $state(1200);
  let hover = $state(-1);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const pageWidth = $derived(Math.max(260, Math.min(width - 32, 1100) * zoom));

  // Render pages for the shown size × screen density, in steps (so resizing doesn't re-render all the time).
  $effect(() => {
    const want = Math.ceil((pageWidth * (window.devicePixelRatio || 1)) / 300) * 300;
    clearTimeout(timer);
    timer = setTimeout(() => (renderWidth = Math.min(want, 3600)), renderWidth ? 180 : 0);
  });

  $effect(() => {
    const ro = new ResizeObserver(() => (width = scroller.clientWidth));
    ro.observe(scroller);
    width = scroller.clientWidth;
    return () => ro.disconnect();
  });

  const byPage = $derived.by(() => {
    const pages: { i: number; w: (typeof lesson.words)[number] }[][] = lesson.pages.map(() => []);
    lesson.words.forEach((w, i) => pages[w.page]?.push({ i, w }));
    return pages;
  });

  const current = $derived(snap?.phrase ?? -1);
  const sayingPdf = $derived(word >= 0 ? (lesson.spoken[word] ?? -1) : -1);
  const active = $derived(snap?.mode === "teacher" || snap?.mode === "compare");

  // Keep the current phrase in view.
  $effect(() => {
    const k = current;
    if (k < 0 || !scroller) return;
    const first = lesson.words.findIndex((w) => w.phrase === k);
    if (first < 0) return;
    queueMicrotask(() => {
      const el = scroller.querySelector<HTMLElement>(`[data-w="${first}"]`);
      if (!el) return;
      const r = el.getBoundingClientRect();
      const s = scroller.getBoundingClientRect();
      if (r.top < s.top + 40 || r.bottom > s.bottom - 40) {
        scroller.scrollBy({ top: r.top - s.top - s.height * 0.35, behavior: "smooth" });
      }
    });
  });

  // New lesson: back to the top.
  $effect(() => {
    lesson.id;
    if (scroller) scroller.scrollTop = 0;
  });
</script>

<div class="scroller" bind:this={scroller} use:scrollHints>
  {#each lesson.pages as page, p (p)}
    <div class="page" style="width: {pageWidth}px; aspect-ratio: {page.w} / {page.h}">
      <img src={pageUrl(lesson.id, p, renderWidth)} alt="Page {p + 1}" loading={p < 2 ? "eager" : "lazy"} draggable="false" />
      {#each byPage[p] as { i, w } (i)}
        {#if w.phrase >= 0}
          {@const ph = lesson.phrases[w.phrase]}
          <button
            class="w"
            class:cur={w.phrase === current}
            class:say={active && i === sayingPdf}
            class:hov={w.phrase === hover && w.phrase !== current}
            class:done={ph?.done}
            class:marked={marked.has(i)}
            data-w={i}
            style="left: {w.x * 100}%; top: {w.y * 100}%; width: {w.w * 100}%; height: {w.h * 100}%"
            onmouseenter={() => (hover = w.phrase)}
            onmouseleave={() => (hover = -1)}
            onclick={() => onGoto(w.phrase)}
            oncontextmenu={(e) => { e.preventDefault(); onMark(i); }}
            tabindex="-1"
            title={marked.has(i) ? `${ph?.text} (marked: right-click to unmark)` : `${ph?.text} (right-click: mark for attention)`}
            aria-label={`Play: ${ph?.text}`}
          ></button>
        {/if}
      {/each}
    </div>
  {/each}
</div>

<style>
  .scroller {
    position: absolute;
    inset: 0;
    overflow: auto;
    padding: 20px 16px 44px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 22px;
    scroll-behavior: smooth;
    overscroll-behavior: contain;
  }
  .page {
    position: relative;
    flex: none;
    background: #fff;
    box-shadow: var(--page-shadow);
    border-radius: 18px;
  }
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border-radius: 18px;
    pointer-events: none;
  }
  .w {
    position: absolute;
    padding: 0;
    margin: 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    cursor: pointer;
    /* a touch bigger than the glyphs, easier to hit */
    outline: 2px solid transparent;
    transform: scale(1.08, 1.2);
    mix-blend-mode: multiply;
    transition: background 0.08s;
  }
  .w.hov {
    background: rgba(85, 96, 232, 0.2);
    box-shadow: inset 0 -2px 0 rgba(85, 96, 232, 0.65);
  }
  .w.cur {
    background: var(--teacher-soft);
  }
  .w.say {
    background: var(--teacher-word);
  }
  /* marked for attention: an orange tint (done phrases get the green underline) */
  .w.marked {
    background: var(--marked-soft);
  }
  .w.done::after {
    content: "";
    position: absolute;
    left: 4%;
    right: 4%;
    bottom: -3px;
    height: 2.5px;
    border-radius: 2px;
    background: #23a065;
  }
</style>
