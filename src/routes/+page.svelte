<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import type { Settings } from "$lib/settings";

  let settings = $state<Settings | null>(null);
  let status = $state<{ kind: "ok" | "error"; text: string } | null>(null);

  // Why the saved hotkey couldn't be registered at startup (taken, or unsupported on Wayland).
  let hotkeyError = $state<string | null>(null);

  onMount(async () => {
    settings = await invoke<Settings>("get_settings");
    hotkeyError = await invoke<string | null>("hotkey_status");
  });

  async function save(event: Event) {
    event.preventDefault();
    if (!settings) return;
    try {
      settings = await invoke<Settings>("set_settings", { settings });
      hotkeyError = null;
      status = { kind: "ok", text: "Saved" };
    } catch (e) {
      status = { kind: "error", text: String(e) };
    }
  }
</script>

<main>
  <header>
    <div class="orb" aria-hidden="true"></div>
    <div>
      <h1>Morph</h1>
      <p class="sub">Convert files from a radial menu. Everything stays on this computer.</p>
    </div>
  </header>

  {#if settings}
    <form onsubmit={save}>
      <label>
        <span>Hotkey</span>
        <input bind:value={settings.hotkey} placeholder="Ctrl+Shift+Space" />
        <small>Opens the drop orb at your cursor. Press it again to close.</small>
        {#if hotkeyError}
          <small class="warn">
            {hotkeyError} If your desktop blocks app hotkeys (e.g. GNOME on Wayland), add a system
            shortcut that runs <code>morph --toggle</code>.
          </small>
        {/if}
      </label>

      <label>
        <span>File name</span>
        <input bind:value={settings.naming_pattern} />
        <small>
          Converted files are saved in the same folder as the original. <code>{"{name}"}</code> is the
          original name, <code>{"{ext}"}</code> the new extension.
        </small>
      </label>

      <label>
        <span>If the file already exists</span>
        <select bind:value={settings.overwrite}>
          <option value="ask">Ask me</option>
          <option value="rename">Keep both (add a number)</option>
        </select>
      </label>

      <footer>
        {#if status}
          <span class="status {status.kind}">{status.text}</span>
        {/if}
        <button type="submit">Save</button>
      </footer>
    </form>
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
  :global(body) {
    margin: 0;
    background: var(--bg);
  }
  main {
    padding: 24px 28px;
  }
  header {
    display: flex;
    gap: 14px;
    align-items: center;
    margin-bottom: 20px;
  }
  .orb {
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
  form {
    display: grid;
    gap: 16px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 18px;
  }
  label {
    display: grid;
    gap: 6px;
  }
  label > span {
    font-weight: 600;
    margin-bottom: 4px;
  }
  input,
  select {
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 8px 10px;
  }
  input:focus,
  select:focus {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  small {
    color: var(--muted);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 12px;
  }
  button {
    font: inherit;
    font-weight: 600;
    color: #fff;
    background: var(--accent);
    border: 0;
    border-radius: 8px;
    padding: 8px 18px;
    cursor: pointer;
  }
  .status.ok {
    color: var(--accent-2);
  }
  .status.error,
  .warn {
    color: var(--error);
  }
</style>
