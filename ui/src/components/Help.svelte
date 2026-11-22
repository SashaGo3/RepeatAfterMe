<script lang="ts">
  import { scrollHints } from "../lib/scroll";
  import { KEYS, capText } from "../lib/keys";
  let { onClose }: { onClose: () => void } = $props();
  const practice = KEYS.filter((k) => !k.mod && k.action !== "help");
  const app = KEYS.filter((k) => k.mod);
</script>

<div class="backdrop" onclick={onClose} role="presentation"></div>
<div class="sheet card" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts" use:scrollHints>
  <h2>Keyboard shortcuts</h2>
  <p class="muted">Listen, say it, hear both, then <b>you</b> decide: <span class="kbd">↵</span> good, <span class="kbd">R</span> again.</p>
  <div class="cols">
    <section>
      <h3>Practice</h3>
      <dl>
        {#each practice as k (k.action)}
          <dt><span class="kbd">{capText(k)}</span></dt>
          <dd>{k.action === "space" ? "Start · my turn now · done talking · hear both again" : k.label}</dd>
        {/each}
        <dt><span class="kbd">Click</span></dt>
        <dd>a word in the PDF: practise its phrase</dd>
        <dt><span class="kbd">Right-click</span></dt>
        <dd>a word: mark it for attention, in orange (again to unmark)</dd>
      </dl>
    </section>
    <section>
      <h3>App</h3>
      <dl>
        {#each app as k (k.action)}
          <dt><span class="kbd">{capText(k)}</span></dt>
          <dd>{k.label}</dd>
        {/each}
        <dt><span class="kbd">?</span></dt>
        <dd>This sheet</dd>
      </dl>
    </section>
  </div>
  <button class="btn" onclick={onClose}>Close <span class="kbd">Esc</span></button>
</div>

<style>
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
    width: min(720px, calc(100% - 32px));
    max-height: calc(100% - 32px);
    overflow: auto;
    padding: 24px 28px;
    z-index: 41;
  }
  h2 {
    margin: 0 0 4px;
    font-size: 1.25em;
  }
  h3 {
    margin: 14px 0 6px;
    font-size: 0.85em;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .cols {
    display: grid;
    grid-template-columns: 1.3fr 1fr;
    gap: 8px 28px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px 12px;
    margin: 0 0 14px;
    align-items: center;
  }
  dd {
    margin: 0;
  }
  dt {
    text-align: right;
  }
  @media (max-width: 600px) {
    .cols {
      grid-template-columns: 1fr;
    }
  }
</style>
