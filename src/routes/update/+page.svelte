<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  // Mirrors `updater::Info` in src-tauri/src/updater.rs.
  interface Info {
    current: string;
    version: string;
    notes: string | null;
  }

  type View =
    | { kind: "checking" }
    | { kind: "current" }
    | { kind: "available"; info: Info }
    | { kind: "installing"; info: Info; downloaded: number; total: number | null }
    | { kind: "error"; text: string; info: Info | null };

  let view = $state<View>({ kind: "checking" });

  const close = () => getCurrentWindow().close();
  const mb = (n: number) => (n / 1048576).toFixed(1);

  async function check() {
    view = { kind: "checking" };
    try {
      const info = await invoke<Info | null>("update_check");
      view = info ? { kind: "available", info } : { kind: "current" };
    } catch (e) {
      view = { kind: "error", text: String(e), info: null };
    }
  }

  async function install(info: Info) {
    view = { kind: "installing", info, downloaded: 0, total: null };
    try {
      await invoke("update_install"); // Morph restarts when it's done
    } catch (e) {
      view = { kind: "error", text: String(e), info };
    }
  }

  async function skip(info: Info) {
    await invoke("update_skip", { version: info.version });
    close();
  }

  onMount(() => {
    check();
    const unlisten = listen<{ downloaded: number; total: number | null }>("update-progress", (e) => {
      if (view.kind === "installing") view = { ...view, ...e.payload };
    });
    return () => unlisten.then((f) => f());
  });
</script>

<main>
  <header>
    <div class="orb" aria-hidden="true"></div>
    <div>
      {#if view.kind === "available" || view.kind === "installing"}
        <h1>Morph {view.info.version} is available</h1>
        <p class="sub">You have {view.info.current}.</p>
      {:else if view.kind === "current"}
        <h1>Morph is up to date</h1>
        <p class="sub">You have the latest version.</p>
      {:else if view.kind === "error"}
        <h1>Couldn't update</h1>
        <p class="sub">{view.info ? `Morph ${view.info.version} is available.` : "Couldn't check for updates."}</p>
      {:else}
        <h1>Checking for updates…</h1>
        <p class="sub">Asking GitHub for the latest version.</p>
      {/if}
    </div>
  </header>

  <section>
    {#if view.kind === "available"}
      <h2>What's new</h2>
      <pre>{view.info.notes || "Improvements and fixes."}</pre>
    {:else if view.kind === "installing"}
      <p>{view.total ? `Downloading… ${mb(view.downloaded)} of ${mb(view.total)} MB` : "Downloading…"}</p>
      <div class="bar"><div style:width={view.total ? `${(100 * view.downloaded) / view.total}%` : "30%"}></div></div>
      <p class="muted">Morph restarts by itself when the update is installed.</p>
    {:else if view.kind === "error"}
      <p class="warn">{view.text}</p>
      <p class="muted">You can download the new version from the Morph website or GitHub instead.</p>
    {:else if view.kind === "current"}
      <p class="muted">Morph looks for updates each time it starts. You can turn that off in Settings.</p>
    {/if}
  </section>

  <footer>
    {#if view.kind === "available"}
      {@const info = view.info}
      <button class="link" onclick={() => skip(info)}>Skip this version</button>
      <span class="spacer"></span>
      <button class="secondary" onclick={close}>Later</button>
      <button onclick={() => install(info)}>Update and restart</button>
    {:else if view.kind === "error"}
      <span class="spacer"></span>
      <button class="secondary" onclick={() => invoke("update_open_downloads")}>Download page</button>
      <button onclick={check}>Try again</button>
    {:else if view.kind !== "installing"}
      <span class="spacer"></span>
      <button onclick={close} disabled={view.kind === "checking"}>OK</button>
    {/if}
  </footer>
</main>

<style>
  :root {
    --bg: #f6f5f2;
    --panel: #ffffff;
    --text: #1d1c1a;
    --muted: #6b6862;
    --border: #e2dfd8;
    --accent: #6c4bf4;
    --accent-2: #19c3b2;
    --error: #c2362b;
    font-family: "Segoe UI", system-ui, -apple-system, Ubuntu, Cantarell, sans-serif;
    font-size: 14px;
    color: var(--text);
    background: var(--bg);
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --bg: #17161a;
      --panel: #211f25;
      --text: #ecebef;
      --muted: #9b98a3;
      --border: #34313a;
      --error: #ff7a6e;
    }
  }
  :global(html),
  :global(body) {
    height: 100%;
    margin: 0;
    overflow: hidden;
    background: var(--bg);
  }
  /* Fills the window at any system text size; the notes scroll, the buttons stay visible. */
  main {
    box-sizing: border-box;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 20px 24px;
  }
  header {
    display: flex;
    gap: 14px;
    align-items: center;
    margin-bottom: 16px;
  }
  .orb {
    flex: none;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: conic-gradient(from 200deg, var(--accent), var(--accent-2), var(--accent));
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 18%, transparent);
  }
  h1 {
    margin: 0;
    font-size: 18px;
  }
  .sub {
    margin: 2px 0 0;
    color: var(--muted);
  }
  section {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 14px 18px;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 14px;
  }
  pre {
    margin: 0;
    font: inherit;
    white-space: pre-wrap;
    color: var(--muted);
    line-height: 1.45;
  }
  p {
    margin: 0 0 8px;
  }
  p:last-child {
    margin-bottom: 0;
  }
  .muted {
    color: var(--muted);
  }
  .warn {
    color: var(--error);
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--border);
    overflow: hidden;
    margin-bottom: 10px;
  }
  .bar > div {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }
  footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: auto;
    padding-top: 14px;
  }
  .spacer {
    flex: 1;
  }
  button {
    font: inherit;
    font-weight: 600;
    color: #fff;
    background: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 8px;
    padding: 8px 18px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.secondary {
    color: var(--accent);
    background: transparent;
  }
  button.link {
    color: var(--muted);
    background: none;
    border: 0;
    padding: 8px 0;
    font-weight: 400;
    text-decoration: underline;
  }
</style>
