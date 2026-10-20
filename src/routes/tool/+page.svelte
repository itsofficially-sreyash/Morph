<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  // Mirrors toolwin.rs.
  type Kind = "merge" | "split" | "resize" | "crop";
  interface FileInfo {
    path: string;
    name: string;
    pages: number | null;
    width: number | null;
    height: number | null;
    error: string | null;
  }
  interface Summary {
    ok: boolean;
    message: string;
    saved: string[];
  }

  let kind = $state<Kind | null>(null);
  let files = $state<FileInfo[]>([]);
  let loadError = $state<string | null>(null);
  let busy = $state(false);
  let result = $state<Summary | null>(null);

  const usable = $derived(files.filter((f) => !f.error));

  onMount(async () => {
    try {
      const info = await invoke<{ kind: Kind; files: FileInfo[] }>("tool_info");
      kind = info.kind;
      files = info.files;
      if (kind === "crop") loadPreview();
    } catch (e) {
      loadError = String(e);
    }
  });

  function close() {
    getCurrentWindow().close();
  }

  async function run(list: string[], params: object, closeAfter = true) {
    busy = true;
    result = null;
    try {
      result = await invoke<Summary>("tool_run", { files: list, params });
    } catch (e) {
      result = { ok: false, message: String(e), saved: [] };
    }
    busy = false;
    if (result.ok && closeAfter) setTimeout(close, 1400);
    return result.ok;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !busy) close();
  }

  // ---- Merge: order the PDFs ----
  let dragFrom = $state<number | null>(null);
  function move(from: number, to: number) {
    if (to < 0 || to >= files.length || from === to) return;
    const next = [...files];
    const [item] = next.splice(from, 1);
    next.splice(to, 0, item);
    files = next;
  }
  function remove(i: number) {
    files = files.filter((_, j) => j !== i);
  }
  const totalPages = $derived(usable.reduce((n, f) => n + (f.pages ?? 0), 0));

  // ---- Split: every page, or ranges ----
  let splitMode = $state<"every" | "ranges">("every");
  let rangesText = $state("");
  let ranges = $state<[number, number][]>([]);
  let rangesError = $state<string | null>(null);
  const splitFile = $derived(usable[0]);
  $effect(() => {
    const text = rangesText;
    const pages = splitFile?.pages ?? 0;
    if (splitMode !== "ranges" || !pages) return;
    invoke<[number, number][]>("tool_parse_ranges", { text, pages })
      .then((r) => {
        ranges = r;
        rangesError = null;
      })
      .catch((e) => {
        ranges = [];
        rangesError = text.trim() ? String(e) : null;
      });
  });
  const label = (r: [number, number]) => (r[0] === r[1] ? `page ${r[0]}` : `pages ${r[0]}–${r[1]}`);

  // ---- Resize: percentage or size ----
  let resizeMode = $state<"percent" | "size">("percent");
  let percent = $state(50);
  let width = $state<number | null>(null);
  let height = $state<number | null>(null);
  let lock = $state(true);
  let lastEdited = $state<"width" | "height">("width");
  const one = $derived(usable.length === 1 ? usable[0] : null);
  function editWidth(v: number | null) {
    width = v;
    lastEdited = "width";
    if (lock && one && v) height = Math.max(1, Math.round((v * one.height!) / one.width!));
  }
  function editHeight(v: number | null) {
    height = v;
    lastEdited = "height";
    if (lock && one && v) width = Math.max(1, Math.round((v * one.width!) / one.height!));
  }
  /** With the lock on, send only the side the user typed so each image keeps its shape. */
  const resizeParams = $derived(
    resizeMode === "percent"
      ? { kind: "resize", percent }
      : lock
        ? lastEdited === "width"
          ? { kind: "resize", width }
          : { kind: "resize", height }
        : { kind: "resize", width, height },
  );
  const resizeReady = $derived(
    resizeMode === "percent" ? percent > 0 && percent <= 1000 : lock ? !!(lastEdited === "width" ? width : height) : !!(width && height),
  );
  const resizePreview = $derived.by(() => {
    if (!one?.width || !one.height) return null;
    if (resizeMode === "percent") return [Math.round((one.width * percent) / 100), Math.round((one.height * percent) / 100)];
    if (width && height) return [width, height];
    return null;
  });

  // ---- Crop: draw a box ----
  let cropIndex = $state(0);
  let preview = $state<string | null>(null);
  let previewError = $state<string | null>(null);
  let imgEl = $state<HTMLImageElement>();
  let shown = $state({ w: 0, h: 0 }); // displayed image size in CSS px
  let box = $state({ x: 0, y: 0, w: 0, h: 0 }); // in displayed px
  let ratio = $state<number | null>(null); // width / height, null = free
  const cropFile = $derived(usable[cropIndex]);
  const scale = $derived(cropFile?.width && shown.w ? cropFile.width / shown.w : 1);
  const realBox = $derived({
    x: Math.round(box.x * scale),
    y: Math.round(box.y * scale),
    width: Math.max(1, Math.round(box.w * scale)),
    height: Math.max(1, Math.round(box.h * scale)),
  });

  async function loadPreview() {
    preview = null;
    previewError = null;
    const f = usable[cropIndex];
    if (!f) return;
    try {
      preview = await invoke<string>("tool_preview", { path: f.path });
    } catch (e) {
      previewError = String(e);
    }
  }
  function onImageLoad() {
    if (!imgEl) return;
    shown = { w: imgEl.clientWidth, h: imgEl.clientHeight };
    resetBox();
  }
  function resetBox() {
    // Start with a centred box covering 80% (respecting the chosen ratio).
    let w = shown.w * 0.8;
    let h = shown.h * 0.8;
    if (ratio) {
      if (w / h > ratio) w = h * ratio;
      else h = w / ratio;
    }
    box = { x: (shown.w - w) / 2, y: (shown.h - h) / 2, w, h };
  }
  function setRatio(r: number | null) {
    ratio = r;
    resetBox();
  }
  const ratios: [string, number | null][] = [
    ["Free", null],
    ["1:1", 1],
    ["4:3", 4 / 3],
    ["3:2", 3 / 2],
    ["16:9", 16 / 9],
  ];

  type Grip = "move" | "nw" | "ne" | "sw" | "se";
  let drag: { grip: Grip; sx: number; sy: number; start: typeof box } | null = null;
  function startDrag(e: PointerEvent, grip: Grip) {
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    drag = { grip, sx: e.clientX, sy: e.clientY, start: { ...box } };
  }
  function onDrag(e: PointerEvent) {
    if (!drag) return;
    const dx = e.clientX - drag.sx;
    const dy = e.clientY - drag.sy;
    const s = drag.start;
    const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);
    const MIN = 16;
    if (drag.grip === "move") {
      box = { ...s, x: clamp(s.x + dx, 0, shown.w - s.w), y: clamp(s.y + dy, 0, shown.h - s.h) };
      return;
    }
    // Resize from a corner; the opposite corner stays put.
    const left = drag.grip.includes("w");
    const top = drag.grip.includes("n");
    const fx = left ? s.x + s.w : s.x;
    const fy = top ? s.y + s.h : s.y;
    let w = clamp(left ? s.w - dx : s.w + dx, MIN, left ? fx : shown.w - fx);
    let h = clamp(top ? s.h - dy : s.h + dy, MIN, top ? fy : shown.h - fy);
    if (ratio) {
      if (w / h > ratio) w = h * ratio;
      else h = w / ratio;
    }
    box = { x: left ? fx - w : fx, y: top ? fy - h : fy, w, h };
  }
  function endDrag() {
    drag = null;
  }
  async function saveCrop() {
    const f = cropFile;
    if (!f) return;
    const last = cropIndex >= usable.length - 1;
    const ok = await run([f.path], { kind: "crop", ...realBox }, last);
    if (ok && !last) nextCrop();
  }
  function nextCrop() {
    cropIndex += 1;
    if (cropIndex >= usable.length) close();
    else loadPreview();
  }
</script>

<svelte:window {onkeydown} onpointermove={onDrag} onpointerup={endDrag} />

<main class:wide={kind === "crop"}>
  {#if loadError}
    <p class="error">{loadError}</p>
  {:else if !kind}
    <p class="muted">Loading…</p>
  {:else if kind === "merge"}
    <h1>Merge PDFs</h1>
    <p class="muted">Drag to set the order. The merged PDF is saved next to the first one.</p>
    <ol class="list">
      {#each files as f, i (f.path)}
        <li
          class:bad={!!f.error}
          class:dragging={dragFrom === i}
          draggable="true"
          ondragstart={() => (dragFrom = i)}
          ondragover={(e) => e.preventDefault()}
          ondrop={() => {
            if (dragFrom !== null) move(dragFrom, i);
            dragFrom = null;
          }}
          ondragend={() => (dragFrom = null)}
        >
          <span class="grip" aria-hidden="true">⋮⋮</span>
          <span class="num">{i + 1}</span>
          <span class="name" title={f.path}>{f.name}</span>
          <span class="meta">{f.error ?? `${f.pages} page${f.pages === 1 ? "" : "s"}`}</span>
          <span class="btns">
            <button class="icon" title="Move up" disabled={i === 0} onclick={() => move(i, i - 1)}>↑</button>
            <button class="icon" title="Move down" disabled={i === files.length - 1} onclick={() => move(i, i + 1)}>↓</button>
            <button class="icon" title="Leave out" onclick={() => remove(i)}>✕</button>
          </span>
        </li>
      {/each}
    </ol>
    <footer>
      <span class="muted">{usable.length} PDFs · {totalPages} pages</span>
      <button class="primary" disabled={busy || usable.length < 2} onclick={() => run(usable.map((f) => f.path), { kind: "none" })}>
        {busy ? "Merging…" : "Merge"}
      </button>
    </footer>
  {:else if kind === "split"}
    <h1>Split PDF</h1>
    {#if splitFile}
      <p class="muted">{splitFile.name} · {splitFile.pages} pages{usable.length > 1 ? ` (and ${usable.length - 1} more)` : ""}</p>
      <label class="radio">
        <input type="radio" bind:group={splitMode} value="every" />
        Every page as its own PDF
        <span class="muted">({usable.reduce((n, f) => n + (f.pages ?? 0), 0)} files)</span>
      </label>
      <label class="radio" class:off={usable.length > 1}>
        <input type="radio" bind:group={splitMode} value="ranges" disabled={usable.length > 1} />
        Custom ranges
      </label>
      {#if splitMode === "ranges"}
        <input class="ranges" placeholder="e.g. 1-3, 5, 8-" bind:value={rangesText} />
        {#if rangesError}
          <p class="error small">{rangesError}</p>
        {:else if ranges.length}
          <p class="small">{ranges.length} file{ranges.length === 1 ? "" : "s"}: {ranges.map(label).join(" · ")}</p>
        {:else}
          <p class="muted small">Each range becomes one PDF. “8-” means page 8 to the end.</p>
        {/if}
      {/if}
      <footer>
        <span></span>
        <button
          class="primary"
          disabled={busy || (splitMode === "ranges" && !ranges.length)}
          onclick={() => run(usable.map((f) => f.path), { kind: "split", ranges: splitMode === "ranges" ? ranges : [] })}
        >
          {busy ? "Splitting…" : "Split"}
        </button>
      </footer>
    {:else}
      <p class="error">{files[0]?.error ?? "No PDF to split"}</p>
    {/if}
  {:else if kind === "resize"}
    <h1>Resize</h1>
    <p class="muted">
      {#if one}{one.name} · {one.width} × {one.height}{:else}{usable.length} images{/if}
    </p>
    <div class="tabs">
      <button class:on={resizeMode === "percent"} onclick={() => (resizeMode = "percent")}>Percentage</button>
      <button class:on={resizeMode === "size"} onclick={() => (resizeMode = "size")}>Size in pixels</button>
    </div>
    {#if resizeMode === "percent"}
      <div class="row">
        {#each [25, 50, 75, 150, 200] as p}
          <button class="chip" class:on={percent === p} onclick={() => (percent = p)}>{p}%</button>
        {/each}
        <input class="num-input" type="number" min="1" max="1000" bind:value={percent} />%
      </div>
    {:else}
      <div class="row">
        <label class="field">Width <input type="number" min="1" value={width} oninput={(e) => editWidth(+e.currentTarget.value || null)} /></label>
        <button class="icon lock" class:on={lock} title="Keep proportions" onclick={() => (lock = !lock)}>{lock ? "🔗" : "⛓️‍💥"}</button>
        <label class="field">Height <input type="number" min="1" value={height} oninput={(e) => editHeight(+e.currentTarget.value || null)} /></label>
      </div>
      <p class="muted small">
        {lock ? "Proportions are kept: type a width or a height." : "Exact size: the image may be stretched."}
      </p>
    {/if}
    {#if resizePreview}
      <p class="small">New size: {resizePreview[0]} × {resizePreview[1]}</p>
    {/if}
    <footer>
      <span></span>
      <button class="primary" disabled={busy || !resizeReady} onclick={() => run(usable.map((f) => f.path), resizeParams)}>
        {busy ? "Resizing…" : "Resize"}
      </button>
    </footer>
  {:else if kind === "crop"}
    <header class="crop-head">
      <h1>Crop</h1>
      {#if cropFile}
        <span class="muted">{cropFile.name}{usable.length > 1 ? ` · ${cropIndex + 1} of ${usable.length}` : ""}</span>
      {/if}
      <span class="spacer"></span>
      {#each ratios as [name, r]}
        <button class="chip" class:on={ratio === r} onclick={() => setRatio(r)}>{name}</button>
      {/each}
    </header>
    <div class="stage">
      {#if previewError}
        <p class="error">{previewError}</p>
      {:else if !preview}
        <p class="muted">Loading the image…</p>
      {:else}
        <div class="canvas">
          <img bind:this={imgEl} src={preview} alt="" onload={onImageLoad} draggable="false" />
          <!-- Dim outside the box; the box itself moves, its corners resize it. -->
          <div
            class="box"
            role="presentation"
            style="left:{box.x}px;top:{box.y}px;width:{box.w}px;height:{box.h}px"
            onpointerdown={(e) => startDrag(e, "move")}
          >
            {#each ["nw", "ne", "sw", "se"] as g}
              <span class="handle {g}" role="presentation" onpointerdown={(e) => startDrag(e, g as Grip)}></span>
            {/each}
          </div>
        </div>
      {/if}
    </div>
    <footer>
      <span class="muted">{realBox.width} × {realBox.height} px</span>
      <span class="spacer"></span>
      <button onclick={resetBox} disabled={!preview || busy}>Reset</button>
      {#if usable.length > 1 && cropIndex < usable.length - 1}
        <button onclick={nextCrop} disabled={busy}>Skip</button>
      {/if}
      <button class="primary" onclick={saveCrop} disabled={!preview || busy}>
        {busy ? "Saving…" : usable.length > 1 && cropIndex < usable.length - 1 ? "Save & next" : "Save"}
      </button>
    </footer>
  {/if}

  {#if result}
    <p class="result" class:ok={result.ok} class:error={!result.ok}>{result.ok ? "✓ " : ""}{result.message}</p>
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
    --ok: #138a7e;
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
      --accent: #9b82ff;
      --ok: #3fd1c0;
      --error: #ff7a6e;
    }
  }
  :global(body) {
    margin: 0;
    background: var(--bg);
  }
  main {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 20px 22px;
    height: 100vh;
    box-sizing: border-box;
  }
  h1 {
    margin: 0;
    font-size: 18px;
  }
  p {
    margin: 0;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 12px;
  }
  .error {
    color: var(--error);
  }
  .result {
    font-weight: 600;
  }
  .result.ok {
    color: var(--ok);
  }
  button {
    font: inherit;
    color: var(--text);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 6px 12px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  button.primary {
    color: #fff;
    background: var(--accent);
    border-color: transparent;
    font-weight: 600;
    padding: 8px 20px;
  }
  button.icon {
    padding: 2px 8px;
  }
  button.on,
  .chip.on {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }
  input {
    font: inherit;
    color: var(--text);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 7px 10px;
  }
  input:focus {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-top: auto;
  }
  .spacer {
    flex: 1;
  }
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  .tabs {
    display: flex;
    gap: 6px;
  }
  .num-input {
    width: 70px;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .field input {
    width: 90px;
  }
  .radio {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .radio.off {
    opacity: 0.5;
  }
  .ranges {
    width: 100%;
    box-sizing: border-box;
  }

  /* Merge list */
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow-y: auto;
  }
  .list li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    cursor: grab;
  }
  .list li.dragging {
    opacity: 0.5;
  }
  .list li.bad {
    opacity: 0.6;
  }
  .list li.bad .meta {
    color: var(--error);
  }
  .grip {
    color: var(--muted);
    letter-spacing: -2px;
  }
  .num {
    font-weight: 700;
    color: var(--accent);
    width: 1.5em;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
  }
  .btns {
    display: flex;
    gap: 4px;
  }

  /* Crop */
  .crop-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .stage {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    background: repeating-conic-gradient(var(--border) 0 25%, var(--panel) 0 50%) 0 0 / 20px 20px;
    border-radius: 10px;
    overflow: hidden;
  }
  .canvas {
    position: relative;
    max-width: 100%;
    max-height: 100%;
    line-height: 0;
  }
  .canvas img {
    max-width: calc(100vw - 60px);
    max-height: calc(100vh - 170px);
    user-select: none;
  }
  .box {
    position: absolute;
    border: 2px solid #fff;
    box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.5);
    outline: 1px solid rgba(0, 0, 0, 0.4);
    cursor: move;
    touch-action: none;
  }
  .handle {
    position: absolute;
    width: 14px;
    height: 14px;
    background: #fff;
    border: 1px solid rgba(0, 0, 0, 0.5);
    border-radius: 3px;
  }
  .handle.nw {
    left: -8px;
    top: -8px;
    cursor: nwse-resize;
  }
  .handle.ne {
    right: -8px;
    top: -8px;
    cursor: nesw-resize;
  }
  .handle.sw {
    left: -8px;
    bottom: -8px;
    cursor: nesw-resize;
  }
  .handle.se {
    right: -8px;
    bottom: -8px;
    cursor: nwse-resize;
  }
</style>
