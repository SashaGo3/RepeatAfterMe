<script lang="ts">
  import { onMount } from "svelte";
  import MicCheck from "./MicCheck.svelte";
  import AddLesson from "./AddLesson.svelte";
  import { api, type DeviceList, type Settings } from "../lib/api";
  import { keyLayer, pressFocused } from "../lib/keys";

  let { settings, micLevel, onChange, onAdd, onFolder, dropped = null }: {
    settings: Settings | null;
    micLevel: number;
    onChange: (s: Settings) => void;
    onAdd: (paths: string[]) => void;
    onFolder: () => void;
    dropped?: string[] | null;
  } = $props();

  const STEPS = ["Welcome", "Microphone", "Speakers", "Lessons"];
  let step = $state(0);
  let devices = $state<DeviceList>({ inputs: [], outputs: [] });

  const back = () => (step = Math.max(0, step - 1));
  const next = () => (step = Math.min(STEPS.length - 1, step + 1));

  onMount(() => {
    api.devices().then((d) => (devices = d));
    // ← → (and Enter) move between the steps; Space plays the test sound on the speakers step
    return keyLayer((e) => {
      if (pressFocused(e)) return true;
      switch (e.key) {
        case "ArrowRight":
        case "Enter":
          next();
          return true;
        case "ArrowLeft":
          back();
          return true;
        case " ":
          if (step === 2 && !e.repeat) api.testSound();
          return true;
      }
      return false;
    });
  });

  // the level meter runs only on the microphone step
  $effect(() => {
    if (step === 1) {
      api.micTest(true);
      return () => api.micTest(false);
    }
  });

  // dropping files jumps to the lessons step
  $effect(() => {
    if (dropped?.length) step = 3;
  });

  const set = (patch: Partial<Settings>) => settings && onChange({ ...settings, ...patch });
  const bars = 24;
</script>

<div class="welcome">
  <div class="card box">
    <ol class="dots" aria-label="Setup steps">
      {#each STEPS as s, i (s)}
        <li>
          <button class:on={i === step} class:past={i < step} onclick={() => (step = i)} aria-current={i === step ? "step" : undefined}>
            <span class="n">{i < step ? "✓" : i + 1}</span><span class="lbl">{s}</span>
          </button>
        </li>
      {/each}
    </ol>

    <div class="page">
      {#if step === 0}
        <h1>Repeat after your teacher</h1>
        <p class="lead">Hear a phrase, say it, then hear <b>the teacher and yourself</b> back to back. <b>You</b> decide when it's good.</p>
        <div class="offline">
          <div class="ic" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="20" height="20"><path d="M2 8.5a15 15 0 0 1 20 0M5.5 12a10 10 0 0 1 13 0M9 15.5a5 5 0 0 1 6 0" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" /><circle cx="12" cy="19" r="1.4" fill="currentColor" /><path d="M3 3l18 18" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" /></svg>
          </div>
          <div class="txt">
            <b>No internet, ever</b>
            <span class="muted">Everything runs on this computer. Your lessons and your voice never leave it.</span>
          </div>
        </div>
        <div class="offline">
          <div class="ic" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="20" height="20"><path d="M6 3h8l4 4v14H6z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" /><path d="M9 12h6M9 16h4" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg>
          </div>
          <div class="txt">
            <b>Your own course</b>
            <span class="muted">Add a lesson's recording and its PDF. The words light up in the PDF as the teacher says them.</span>
          </div>
        </div>
      {:else if step === 1}
        <h1>Your microphone</h1>
        <p class="lead">The app listens while you repeat, so it needs your microphone.</p>
        <MicCheck />
        {#if settings}
          <select class="field" value={settings.input ?? ""} onchange={(e) => set({ input: (e.currentTarget as HTMLSelectElement).value || null })}>
            <option value="">System default microphone</option>
            {#each devices.inputs as d, i (i)}<option value={d}>{d}</option>{/each}
          </select>
        {/if}
        <div class="meter" aria-label="Microphone level">
          {#each Array(bars) as _, i (i)}<i class:on={i < Math.round(micLevel * bars)}></i>{/each}
        </div>
        <p class="hint muted">Say "hello": the bars should jump. A headset or a mic close to you works best.</p>
      {:else if step === 2}
        <h1>Your speakers</h1>
        <p class="lead">Make sure you can hear the teacher.</p>
        <button class="btn primary big" onclick={() => api.testSound()}>
          <svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9v6h4l5 4V5L8 9H4z" fill="currentColor" /><path d="M16 8.5a5 5 0 0 1 0 7M18.5 6a8.5 8.5 0 0 1 0 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg>
          Play "Repeat after me"
          <span class="kbd">Space</span>
        </button>
        {#if settings}
          <select class="field out" value={settings.output ?? ""} onchange={(e) => set({ output: (e.currentTarget as HTMLSelectElement).value || null })}>
            <option value="">System default speakers</option>
            {#each devices.outputs as d, i (i)}<option value={d}>{d}</option>{/each}
          </select>
        {/if}
        <p class="hint muted">Not hearing it? Pick the device you listen on and check its volume. Headphones work best: the microphone won't hear the teacher.</p>
      {:else}
        <h1>Add your lessons</h1>
        <p class="lead">A lesson is the teacher's <b>audio recording</b> and the <b>PDF</b> they read from.</p>
        <AddLesson {onAdd} {onFolder} {dropped} />
      {/if}
    </div>

    <div class="nav">
      <button class="btn" onclick={back} disabled={step === 0}><span class="kbd">←</span> Back</button>
      {#if step < STEPS.length - 1}
        <button class="btn primary" onclick={next}>{step === 0 ? "Let's set up" : "Next"} <span class="kbd">→</span></button>
      {/if}
    </div>
  </div>
</div>

<style>
  .welcome {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 20px;
    overflow: auto;
  }
  .box {
    display: grid;
    gap: 22px;
    width: min(640px, 100%);
    padding: 26px 32px 28px;
  }
  .dots {
    display: flex;
    justify-content: center;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
    flex-wrap: wrap;
  }
  .dots button {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 14px 5px 5px;
    border: 0;
    border-radius: 999px;
    background: var(--bg);
    color: var(--muted);
    font-weight: 600;
    font-size: 0.86em;
    transition: box-shadow 0.2s, color 0.2s;
  }
  .dots button.on {
    box-shadow: var(--raised-xs);
    color: var(--accent);
  }
  .n {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    box-shadow: var(--pressed-sm);
    font-size: 0.85em;
  }
  .past .n {
    color: var(--you);
  }
  .page {
    display: grid;
    gap: 16px;
    min-height: 300px;
    align-content: start;
    text-align: center;
    animation: fade 0.25s ease;
  }
  @keyframes fade {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  h1 {
    margin: 4px 0 0;
    font-size: 1.6em;
    letter-spacing: -0.01em;
    text-wrap: balance;
  }
  .lead {
    margin: -6px 0 4px;
    color: var(--muted);
    text-wrap: balance;
  }
  .lead b {
    color: var(--text);
  }
  .offline {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 14px;
    padding: 14px 16px;
    border-radius: var(--r-lg);
    box-shadow: var(--pressed-sm);
    text-align: left;
  }
  .offline .ic {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    box-shadow: var(--raised-xs);
    color: var(--accent);
  }
  .offline .txt {
    display: grid;
    gap: 2px;
  }
  .offline .muted,
  .hint {
    font-size: 0.87em;
  }
  .hint {
    margin: 0;
    text-wrap: pretty;
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
  .big {
    justify-self: center;
    min-height: 56px;
    padding: 0 26px;
    border-radius: var(--r-lg);
  }
  .nav {
    display: flex;
    justify-content: space-between;
  }
  .nav .btn,
  .big {
    gap: 10px;
  }
  @media (max-width: 560px) {
    .box {
      padding: 20px 16px;
    }
    .lbl {
      display: none;
    }
    .dots button {
      padding: 5px;
    }
  }
</style>
