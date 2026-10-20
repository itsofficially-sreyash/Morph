<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import RadialMenu from "$lib/RadialMenu.svelte";
  import { categoriesFor, fileName, type MenuCategory } from "$lib/mockActions";
  import type { RingItem } from "$lib/radial";

  interface ShownPayload {
    files: string[] | null;
  }

  const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  let files = $state<string[] | null>(null);
  let dragging = $state(false);
  let category = $state<MenuCategory | null>(null);
  let done = $state<string | null>(null);
  let animationKey = $state(0);
  let menu = $state<ReturnType<typeof RadialMenu>>();
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  const available = $derived(files ? categoriesFor(files) : []);
  // With a single kind of file there is nothing to choose at the top level.
  const current = $derived(category ?? (available.length === 1 ? available[0] : null));

  const items = $derived<RingItem[]>(
    done
      ? []
      : current
        ? current.actions.map((a) => ({ id: a.id, label: a.label, hint: a.hint, hue: current.hue }))
        : available.map((c) => ({ id: c.id, label: c.label, hint: `${c.actions.length} actions`, hue: c.hue })),
  );

  const filesLine = $derived.by(() => {
    if (!files?.length) return "";
    const relevant = current
      ? files.filter((f) => current.extensions.includes(f.split(".").pop()?.toLowerCase() ?? ""))
      : files;
    return relevant.length === 1 ? fileName(relevant[0]) : `${relevant.length} files`;
  });

  const title = $derived(
    dragging ? "Let go" : done ? "✓" : files === null ? "Drop files" : (current?.label ?? "Morph"),
  );
  const subtitle = $derived(
    dragging
      ? files?.length
        ? "to add to these files"
        : "to open them in Morph"
      : (done ?? (files === null ? "or press Esc" : available.length ? filesLine : "Nothing to convert")),
  );

  function reset(next: string[] | null) {
    clearTimeout(hideTimer);
    files = next;
    category = null;
    done = null;
    dragging = false;
    animationKey++;
  }

  /** Files dropped on an open orb join the ones already there (no duplicates). */
  function addFiles(dropped: string[]) {
    reset([...new Set([...(files ?? []), ...dropped])]);
  }

  // Browser-only drag and drop, so the drop UI can be worked on without Tauri.
  // Inside Tauri the native handler (overlay://drag, overlay://drop) delivers real paths instead.
  function ondragover(e: DragEvent) {
    if (inTauri) return;
    e.preventDefault();
    dragging = true;
  }
  function ondragleave(e: DragEvent) {
    if (!inTauri && e.target === e.currentTarget) dragging = false;
  }
  function ondrop(e: DragEvent) {
    if (inTauri) return;
    e.preventDefault();
    addFiles([...(e.dataTransfer?.files ?? [])].map((f) => f.name));
  }

  function hide() {
    clearTimeout(hideTimer);
    invoke("hide_overlay");
  }

  function pick(item: RingItem) {
    if (!current) {
      category = available.find((c) => c.id === item.id) ?? null;
      return;
    }
    // Step 4 runs the real job; for now just confirm the choice.
    done = `${item.hint} · preview`;
    hideTimer = setTimeout(hide, 900);
  }

  function back() {
    if (category && available.length > 1) category = null;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      hide();
    } else if (!done && menu?.handleKey(e)) {
      e.preventDefault();
    }
  }

  onMount(() => {
    // Opened in a plain browser (UI work without Tauri): `/overlay?files=a.png,b.mp4`.
    if (!inTauri) {
      const param = new URLSearchParams(location.search).get("files");
      reset(param ? param.split(",") : null);
      return;
    }
    const unlisteners = [
      listen<ShownPayload>("overlay://shown", (e) => reset(e.payload.files)),
      listen<{ over: boolean }>("overlay://drag", (e) => (dragging = e.payload.over)),
      listen<{ files: string[] }>("overlay://drop", (e) => addFiles(e.payload.files)),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((off) => off()));
      clearTimeout(hideTimer);
    };
  });
</script>

<svelte:window {onkeydown} />

<!-- Clicking the transparent corners (outside the ring) closes the orb. -->
<div
  class="stage"
  role="presentation"
  onpointerdown={(e) => e.target === e.currentTarget && hide()}
  {ondragover}
  {ondragleave}
  {ondrop}
>
  <RadialMenu
    bind:this={menu}
    items={dragging ? [] : items}
    glow={dragging}
    {title}
    {subtitle}
    canGoBack={!!category && available.length > 1 && !done}
    {animationKey}
    onpick={pick}
    onback={back}
  />
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    background: transparent;
    overflow: hidden;
  }
  .stage {
    --ring-bg: rgba(24, 22, 30, 0.97);
    --ring-edge: rgba(255, 255, 255, 0.1);
    --hub-hi: #2c2935;
    --hub-lo: #1a1820;
    --slice-l: 26%;
    --slice-a: 0.92;
    --text: #f1eff6;
    --muted: #a19dad;
    --accent: #9b82ff;
    width: 100vw;
    height: 100vh;
  }
  @media (prefers-color-scheme: light) {
    .stage {
      --ring-bg: rgba(250, 249, 246, 0.98);
      --ring-edge: rgba(0, 0, 0, 0.1);
      --hub-hi: #ffffff;
      --hub-lo: #eeece7;
      --slice-l: 90%;
      --slice-a: 1;
      --text: #1d1c1a;
      --muted: #6b6862;
      --accent: #6c4bf4;
    }
  }
</style>
