<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  // Mirrors `welcome::Info` in src-tauri/src/welcome.rs.
  interface Info {
    hotkey: string;
    hotkey_error: string | null;
    os: "windows" | "linux";
    context_menu: boolean;
    start_at_login: boolean;
  }

  let info = $state<Info | null>(null);
  const keys = $derived(info ? info.hotkey.split("+").map((k) => k.trim()) : []);

  onMount(async () => {
    info = await invoke<Info>("welcome_info");
  });

  const close = () => getCurrentWindow().close();
</script>

<main>
  <header>
    <div class="orb" aria-hidden="true"></div>
    <div>
      <h1>Welcome to Morph</h1>
      <p class="sub">Convert and edit files from a radial menu.</p>
    </div>
  </header>

  {#if info}
    <ol>
      <li>
        <span class="n">1</span>
        <div>
          <h2>
            Press
            {#each keys as key, i}{#if i > 0}<span class="plus">+</span>{/if}<kbd>{key}</kbd>{/each}
          </h2>
          <p>The orb opens at your mouse. Drag files onto it, then pick what to do with them.</p>
          {#if info.hotkey_error}
            <p class="warn">
              This shortcut isn't working right now: {info.hotkey_error} You can pick another one in Settings.
            </p>
          {/if}
        </div>
      </li>

      {#if info.context_menu}
        <li>
          <span class="n">2</span>
          <div>
            <h2>Or right-click your files</h2>
            {#if info.os === "windows"}
              <p>In File Explorer, choose <b>Show more options</b>, then <b>Convert with Morph</b>.</p>
            {:else}
              <p>
                In Files, choose <b>Scripts</b>, then <b>Convert with Morph</b>. Other file managers show
                it directly in the menu.
              </p>
            {/if}
          </div>
        </li>
      {/if}

      <li>
        <span class="n">{info.context_menu ? 3 : 2}</span>
        <div>
          <h2>Find the result next to the original</h2>
          <p>Same folder, named like <b>photo.converted.png</b>. Nothing is replaced without asking.</p>
        </div>
      </li>

      <li>
        <span class="n">{info.context_menu ? 4 : 3}</span>
        <div>
          <h2>Nothing leaves your computer</h2>
          <p>
            No account, no uploads. Morph waits in the
            {info.os === "windows" ? "notification area by the clock" : "system tray"}{info.start_at_login
              ? " and starts when you sign in"
              : ""}.
          </p>
        </div>
      </li>
    </ol>

    <footer>
      <button class="link" onclick={() => invoke("welcome_open_settings")}>Settings</button>
      <span class="spacer"></span>
      <button class="secondary" onclick={() => invoke("welcome_try_orb")}>Try it now</button>
      <button onclick={close}>Got it</button>
    </footer>
  {:else}
    <p class="sub">Loading…</p>
  {/if}
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
  /* Fills the window whatever the system text size: the steps scroll if they must, and
     the buttons always stay visible at the bottom. */
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
    font-size: 20px;
  }
  .sub {
    margin: 2px 0 0;
    color: var(--muted);
  }
  ol {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
    list-style: none;
    margin: 0;
    padding: 6px 18px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
  }
  li {
    display: flex;
    gap: 14px;
    padding: 10px 0;
  }
  li + li {
    border-top: 1px solid var(--border);
  }
  .n {
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-size: 12px;
    font-weight: 700;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  h2 {
    margin: 2px 0 4px;
    font-size: 14px;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
  }
  p {
    margin: 0;
    color: var(--muted);
    line-height: 1.45;
  }
  b {
    color: var(--text);
    font-weight: 600;
  }
  kbd {
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    padding: 1px 7px;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 6px;
    background: var(--bg);
  }
  .plus {
    color: var(--muted);
    font-weight: 400;
  }
  .warn {
    margin-top: 6px;
    color: var(--error);
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
