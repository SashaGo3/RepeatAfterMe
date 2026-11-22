<script lang="ts">
  import { scrollHints } from "../lib/scroll";
  import { onMount } from "svelte";
  import MicCheck from "./MicCheck.svelte";
  import { save, ask } from "@tauri-apps/plugin-dialog";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { api, type DeviceList, type Playback, type Settings, type Theme, type TurnEnd } from "../lib/api";

  let { settings, micLevel, onChange, onClose, onCleared, onReset }: {
    settings: Settings;
    micLevel: number;
    onChange: (s: Settings) => void;
    onClose: () => void;
    onCleared?: () => void;
    onReset?: () => void;
  } = $props();

  let devices = $state<DeviceList>({ inputs: [], outputs: [] });

  // the microphone is only opened when you ask for a test (opening Settings alone never makes the
  // system ask for microphone access)
  let testing = $state(false);
  let testTimer: ReturnType<typeof setTimeout> | undefined;
  function test() {
    clearTimeout(testTimer);
    testing = !testing;
    api.micTest(testing);
    if (testing) testTimer = setTimeout(() => { testing = false; api.micTest(false); }, 15000);
  }

  onMount(() => {
    api.devices().then((d) => (devices = d));
    return () => {
      clearTimeout(testTimer);
      if (testing) api.micTest(false);
    };
  });

  const set = (patch: Partial<Settings>) => onChange({ ...settings, ...patch });

  const turnEnds: { v: TurnEnd; label: string; hint: string }[] = [
    { v: "smart", label: "Smart", hint: "Stops a moment after you've said the whole phrase. Pauses mid-phrase are fine." },
    { v: "patient", label: "Patient", hint: "Like Smart, but waits longer. Good if you speak slowly." },
    { v: "manual", label: "When I press Space", hint: "Never stops by itself. Press Space when you're done." },
  ];
  const playbacks: { v: Playback; label: string; hint: string }[] = [
    { v: "teacherThenMe", label: "Teacher, then me", hint: "Hear both back to back to compare." },
    { v: "meOnly", label: "Just me", hint: "Quicker: only your take." },
    { v: "nothing", label: "Nothing", hint: "Fastest: decide right away (T and M still replay)." },
  ];
  const themes: { v: Theme; label: string }[] = [
    { v: "system", label: "System" },
    { v: "light", label: "Light" },
    { v: "dark", label: "Dark" },
  ];
  const bars = 24;
  let exported = $state<string | null>(null);

  async function exportLogs() {
    const day = new Date().toISOString().slice(0, 10);
    const path = await save({ title: "Save the logs", defaultPath: `repeat-after-me-logs-${day}.zip`, filters: [{ name: "Zip", extensions: ["zip"] }] });
    if (!path) return;
    try {
      const n = await api.exportLogs(path);
      exported = `Saved ${n} files to ${path.split(/[\\/]/).pop()}.`;
    } catch (e) {
      exported = `Couldn't save: ${e}`;
    }
  }

  let cleared = $state(false);
  async function resetApp() {
    const yes = await ask("The lesson list, all progress and marked words, your settings, and what the app made from your lessons are deleted, and you start again from the welcome screen. Your own lesson files aren't touched.", { title: "Reset the app?", kind: "warning", okLabel: "Reset the App", cancelLabel: "Cancel" });
    if (!yes) return;
    await api.resetApp();
    onReset?.();
  }
  async function clearAll() {
    const yes = await ask("Every lesson goes back to not done, and time practised to zero. Your marked words stay. This can't be undone (unless you have a backup).", { title: "Clear all progress?", kind: "warning", okLabel: "Clear All Progress", cancelLabel: "Cancel" });
    if (!yes) return;
    await api.resetAll();
    cleared = true;
    onCleared?.();
  }
</script>

<div class="backdrop" onclick={onClose} role="presentation"></div>
<div class="sheet card" role="dialog" aria-modal="true" aria-label="Settings" use:scrollHints>
  <div class="head">
    <h2>Settings</h2>
    <button class="btn round" onclick={onClose} aria-label="Close settings">
      <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M2 2l10 10M12 2L2 12" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" /></svg>
    </button>
  </div>

  <section>
    <h3>Microphone</h3>
    <div class="mic-row">
      <select class="field" value={settings.input ?? ""} onchange={(e) => set({ input: (e.currentTarget as HTMLSelectElement).value || null })}>
        <option value="">System default</option>
        {#each devices.inputs as d, i (i)}<option value={d}>{d}</option>{/each}
      </select>
      <button class="btn" class:on={testing} onclick={test}>{testing ? "Stop" : "Test"}</button>
    </div>
    {#if testing}
      <MicCheck />
      <div class="meter" aria-label="Microphone level">
        {#each Array(bars) as _, i (i)}
          <i class:on={i < Math.round(micLevel * bars)}></i>
        {/each}
      </div>
      <p class="hint">Say something: the bars should move. If not, pick another microphone.</p>
    {:else}
      <p class="hint">Test it to see the level as you speak.</p>
    {/if}
  </section>

  <section>
    <h3>Speakers</h3>
    <select class="field" value={settings.output ?? ""} onchange={(e) => set({ output: (e.currentTarget as HTMLSelectElement).value || null })}>
      <option value="">System default</option>
      {#each devices.outputs as d, i (i)}<option value={d}>{d}</option>{/each}
    </select>
    <p class="hint">Headphones work best: the microphone doesn't hear the teacher.</p>
  </section>

  <section>
    <h3>Your turn ends</h3>
    <div class="seg" role="radiogroup">
      {#each turnEnds as o (o.v)}
        <button class:on={settings.turnEnd === o.v} role="radio" aria-checked={settings.turnEnd === o.v} onclick={() => set({ turnEnd: o.v })}>{o.label}</button>
      {/each}
    </div>
    <p class="hint">{turnEnds.find((o) => o.v === settings.turnEnd)?.hint}</p>
  </section>

  <section>
    <h3>After your turn, play</h3>
    <div class="seg" role="radiogroup">
      {#each playbacks as o (o.v)}
        <button class:on={settings.playback === o.v} role="radio" aria-checked={settings.playback === o.v} onclick={() => set({ playback: o.v })}>{o.label}</button>
      {/each}
    </div>
    <p class="hint">{playbacks.find((o) => o.v === settings.playback)?.hint}</p>
  </section>

  <section>
    <h3>Your answers</h3>
    <button class="toggle-row" onclick={() => set({ saveTakes: !settings.saveTakes })} role="switch" aria-checked={settings.saveTakes}>
      <span class="switch" class:on={settings.saveTakes}></span>
      <span>Save my answers <span class="muted">(to share with your teacher)</span></span>
    </button>
    <p class="hint">Every take is kept on this computer. In a lesson, <b>Export answers</b> makes one audio file: the teacher, then you, phrase by phrase.</p>
  </section>

  <section>
    <h3>Privacy</h3>
    <p class="hint">Repeat After Me never uses the internet. Lessons, your answers and progress stay on this computer{settings.saveTakes ? "" : "; with Save my answers off, your voice is only kept in memory while the app is open"}.</p>
  </section>

  <section>
    <h3>Diagnostics</h3>
    <button class="toggle-row" onclick={() => set({ diagnostics: !settings.diagnostics })} role="switch" aria-checked={settings.diagnostics}>
      <span class="switch" class:on={settings.diagnostics}></span>
      <span>Record diagnostics <span class="muted">(each turn, with its audio)</span></span>
    </button>
    <div class="log-actions">
      <button class="btn" onclick={exportLogs}>Export logs…</button>
      <button class="btn" onclick={async () => openPath(await api.logsDir())}>Show folder</button>
    </div>
    <p class="hint">{exported ?? "Logs stay on this computer. Export them to send when something goes wrong."}</p>
  </section>

  <section>
    <h3>Progress</h3>
    <button class="btn danger" onclick={clearAll}>Clear all progress…</button>
    <p class="hint">{cleared ? "All progress cleared." : "Every lesson back to not done, and time practised to zero. Your marked words stay. To keep a copy first: File → Back Up Progress…"}</p>
    <button class="btn danger" onclick={resetApp}>Reset the app…</button>
    <p class="hint">Back to how it was the first time: no lessons, no progress, default settings. Your lesson files aren't touched.</p>
  </section>

  <section>
    <h3>Appearance</h3>
    <div class="seg" role="radiogroup">
      {#each themes as o (o.v)}
        <button class:on={settings.theme === o.v} role="radio" aria-checked={settings.theme === o.v} onclick={() => set({ theme: o.v })}>{o.label}</button>
      {/each}
    </div>
  </section>
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
    width: min(560px, calc(100% - 32px));
    max-height: calc(100% - 40px);
    overflow: auto;
    padding: 22px 26px 26px;
    z-index: 41;
    display: grid;
    gap: 22px;
    animation: rise 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.1);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, -47%) scale(0.98);
    }
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 1.35em;
  }
  section {
    display: grid;
    gap: 10px;
  }
  h3 {
    margin: 0;
    font-size: 0.78em;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .hint {
    margin: 0;
    font-size: 0.88em;
    color: var(--muted);
    text-wrap: pretty;
  }
  .toggle-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
  }
  .log-actions {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
  }
  .meter {
    display: flex;
    gap: 4px;
    align-items: center;
    height: 30px;
    padding: 0 12px;
    border-radius: 999px;
    box-shadow: var(--pressed-sm);
  }
  .meter i {
    flex: 1;
    height: 8px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--muted) 22%, transparent);
    transition: background 0.08s;
  }
  .meter i.on {
    background: linear-gradient(180deg, var(--you-a), var(--you-b));
  }
  @media (max-width: 520px) {
    .seg {
      flex-direction: column;
    }
  }
  .btn.danger {
    color: #fff;
    background: linear-gradient(145deg, #e5484d, var(--danger));
    box-shadow: var(--raised-sm), 0 0 0 0 transparent;
  }
  .btn.danger:hover:not(:disabled) {
    color: #fff;
    filter: brightness(1.08);
  }
  .mic-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
  }
</style>
