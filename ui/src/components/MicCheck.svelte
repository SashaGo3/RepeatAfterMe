<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type MicState } from "../lib/api";
  import { isMac } from "../lib/keys";

  let { compact = false }: { compact?: boolean } = $props();
  let st = $state<MicState | null>(null);
  let busy = $state(false);

  const isWin = navigator.userAgent.includes("Windows");
  const privacyUrl = isMac
    ? "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
    : isWin
      ? "ms-settings:privacy-microphone"
      : null;

  async function check() {
    if (busy) return;
    busy = true;
    try {
      st = await api.micCheck();
    } catch (e) {
      st = { state: "missing", error: String(e) };
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    check();
    // keep checking until it works (you may be allowing it in the system's settings right now)
    const t = setInterval(() => {
      if (st?.state !== "ok") check();
    }, 2500);
    return () => clearInterval(t);
  });
</script>

<div class="mic" class:compact class:ok={st?.state === "ok"} class:bad={st && st.state !== "ok"}>
  <div class="ic" aria-hidden="true">
    {#if st?.state === "ok"}
      <svg viewBox="0 0 24 24" width="20" height="20"><path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
    {:else}
      <svg viewBox="0 0 24 24" width="20" height="20"><rect x="9" y="3" width="6" height="11" rx="3" fill="none" stroke="currentColor" stroke-width="2" /><path d="M6 11a6 6 0 0 0 12 0M12 17v4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg>
    {/if}
  </div>
  <div class="txt">
    {#if !st}
      <b>Checking the microphone…</b>
    {:else if st.state === "ok"}
      <b>Microphone works</b>
      <span class="muted">{st.device}</span>
    {:else if st.state === "blocked"}
      <b>Microphone access is off</b>
      <span class="muted">Allow it so the app can hear you repeat. Your voice never leaves this computer.</span>
    {:else}
      <b>No microphone found</b>
      <span class="muted">Plug one in (a headset works best).</span>
    {/if}
  </div>
  {#if st && st.state !== "ok"}
    <div class="acts">
      <button class="btn primary" onclick={check} disabled={busy}>{busy ? "Checking…" : "Allow microphone"}</button>
      {#if privacyUrl && st.state === "blocked"}
        <button class="btn" onclick={() => openUrl(privacyUrl)}>Open privacy settings</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .mic {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 6px 14px;
    padding: 14px 16px;
    border-radius: var(--r-lg);
    box-shadow: var(--pressed-sm);
    text-align: left;
  }
  .ic {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    box-shadow: var(--raised-xs);
    color: var(--muted);
  }
  .ok .ic {
    color: var(--you);
  }
  .bad .ic {
    color: var(--danger);
  }
  .txt {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .txt .muted {
    font-size: 0.86em;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .acts {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 6px;
  }
  .acts .btn {
    min-height: 40px;
    font-size: 0.92em;
  }
</style>
