<script lang="ts">
  // A demo of Morph's orb: drag (or tap) a sample file onto it, pick an action, see it "saved".
  // Same geometry, colours and actions as the app's native orb (src-tauri/src/orb, actions.rs).
  // Nothing is converted; the page never touches the visitor's files.
  import { onMount, tick } from 'svelte';

  type Kind = 'convert' | 'tool' | 'off';
  interface Action {
    label: string;
    hint: string;
    kind: Kind;
    /** convert: the saved file's name; tool: what the app opens. */
    result?: string;
  }
  interface Category {
    label: string;
    hue: number;
    actions: Action[];
  }
  interface SampleFile {
    id: string;
    name: string;
    size: string;
    category: Category;
  }

  const conv = (label: string, hint: string, result: string): Action => ({ label, hint, kind: 'convert', result });
  const tool = (label: string, hint: string, result: string): Action => ({ label, hint, kind: 'tool', result });

  const IMAGE: Category = {
    label: 'Image',
    hue: 262,
    actions: [
      conv('PNG', 'Convert to PNG', 'photo.converted.png'),
      conv('JPG', 'Convert to JPG', 'photo.converted.jpg'),
      conv('WEBP', 'Convert to WEBP', 'photo.converted.webp'),
      conv('AVIF', 'Convert to AVIF', 'photo.converted.avif'),
      tool('Resize', 'Resize by size or percentage', 'set the new size'),
      tool('Crop', 'Crop to a region you draw', 'draw the box'),
      conv('Shrink', 'Compress to a smaller file', 'photo.converted.jpg'),
      conv('PDF', 'Put images into one PDF', 'photo.converted.pdf'),
    ],
  };
  const VIDEO: Category = {
    label: 'Video',
    hue: 335,
    actions: [
      conv('MP4', 'Convert to MP4', 'clip.converted.mp4'),
      conv('WEBM', 'Convert to WEBM', 'clip.converted.webm'),
      conv('GIF', 'Convert to GIF', 'clip.converted.gif'),
      conv('Shrink', 'Compress with a quality preset', 'clip.converted.mp4'),
      conv('Audio', 'Extract the soundtrack', 'clip.converted.m4a'),
      conv('Mute', 'Remove the sound', 'clip.converted.mov'),
    ],
  };
  const PDF: Category = {
    label: 'PDF',
    hue: 12,
    actions: [
      { label: 'Merge', hint: 'Drop 2 or more PDF files', kind: 'off' },
      tool('Split', 'Every page, or ranges like 1-3, 5', 'pick the pages'),
      conv('Shrink', 'Make the file smaller (lossless)', 'report.converted.pdf'),
      conv('Images', 'Save each page as a PNG', 'report.converted - page 1.png'),
      conv('Rotate', 'Rotate all pages 90°', 'report.converted.pdf'),
    ],
  };
  const FILES: SampleFile[] = [
    { id: 'photo', name: 'photo.heic', size: '3.1 MB', category: IMAGE },
    { id: 'clip', name: 'clip.mov', size: '48 MB', category: VIDEO },
    { id: 'report', name: 'report.pdf', size: '1.2 MB', category: PDF },
  ];
  const GHOST_HUES = [262, 335, 172, 12, 210];

  // Geometry in the orb's own units (orb/geometry.rs).
  const SIZE = 380;
  const C = SIZE / 2;
  const HUB = 56;
  const R0 = 66;
  const R1 = 170;
  const LABEL_R = (R0 + R1) / 2;
  const BACKDROP_R = R1 + 6;

  function polar(r: number, deg: number): [number, number] {
    const rad = ((deg - 90) * Math.PI) / 180;
    return [C + r * Math.cos(rad), C + r * Math.sin(rad)];
  }

  function sectors(count: number, gap = 2) {
    const span = 360 / count;
    return Array.from({ length: count }, (_, i) => {
      const mid = i * span;
      const half = (span - (count > 1 ? gap : 0)) / 2;
      return { start: mid - half, end: mid + half, mid };
    });
  }

  function annular(r0: number, r1: number, start: number, end: number) {
    const large = end - start > 180 ? 1 : 0;
    const [ax, ay] = polar(r1, start);
    const [bx, by] = polar(r1, end);
    const [cx, cy] = polar(r0, end);
    const [dx, dy] = polar(r0, start);
    const f = (n: number) => n.toFixed(2);
    return `M${f(ax)} ${f(ay)}A${r1} ${r1} 0 ${large} 1 ${f(bx)} ${f(by)}L${f(cx)} ${f(cy)}A${r0} ${r0} 0 ${large} 0 ${f(dx)} ${f(dy)}Z`;
  }

  const hsl = (h: number, s: number, l: number, a = 1) => `hsl(${h} ${s}% ${l}% / ${a})`;

  type Phase = 'idle' | 'menu' | 'working' | 'done' | 'tool';
  let phase = $state<Phase>('idle');
  let file = $state<SampleFile | null>(null);
  let active = $state<number | null>(null);
  let picked = $state<Action | null>(null);
  let openCount = $state(0);
  let tried = $state(false);
  let announce = $state('');

  // Dragging a sample card.
  let dragId = $state<string | null>(null);
  let dragX = $state(0);
  let dragY = $state(0);
  let over = $state(false);
  /** Cards that are "inside" the orb right now (hidden at their spot). */
  let consumed = $state<string | null>(null);
  let flying = $state<{ id: string; x: number; y: number } | null>(null);

  let stage: HTMLDivElement;
  let orbEl: HTMLDivElement;
  let ringEl = $state<SVGGElement>();
  let visible = $state(true);
  let reduced = false;

  let timers: number[] = [];
  const later = (ms: number, fn: () => void) => timers.push(window.setTimeout(fn, ms));
  const clearTimers = () => {
    timers.forEach(clearTimeout);
    timers = [];
  };

  const items = $derived(file ? file.category.actions : []);
  const slices = $derived(items.length ? sectors(items.length) : []);
  const ghost = sectors(GHOST_HUES.length, 3);

  const title = $derived.by(() => {
    if (over) return 'Let go';
    if (phase === 'working') return 'Working…';
    if (phase === 'done') return '';
    if (phase === 'tool' && picked) return picked.label;
    if (phase === 'menu' && file) return file.category.label;
    return 'Drop a file';
  });

  const subtitle = $derived.by(() => {
    if (over) return 'to open it in Morph';
    if (phase === 'working' && picked) return `${picked.hint}…`;
    if (phase === 'done' && picked) return `Saved ${picked.result}`;
    if (phase === 'tool' && picked) return `A window opens to ${picked.result}`;
    if (phase === 'menu' && file) {
      if (active !== null) return items[active].hint;
      return file.name;
    }
    return 'to try it here';
  });

  function reset() {
    clearTimers();
    phase = 'idle';
    file = null;
    active = null;
    picked = null;
    consumed = null;
    flying = null;
  }

  async function open(f: SampleFile, viaKeyboard: boolean) {
    clearTimers();
    file = f;
    consumed = f.id;
    flying = null;
    active = null;
    picked = null;
    phase = 'menu';
    openCount++;
    announce = `${f.name} is in the orb. ${f.category.actions.length} actions. Use the arrow keys or 1 to ${f.category.actions.length}.`;
    // On a phone the cards sit below the orb; bring the whole orb into view.
    const o = orbEl.getBoundingClientRect();
    if (o.top < 70 || o.bottom > innerHeight) {
      orbEl.scrollIntoView({ block: 'center', behavior: reduced ? 'auto' : 'smooth' });
    }
    if (viaKeyboard) {
      await tick();
      ringEl?.querySelector<SVGGElement>('[data-slice]')?.focus({ preventScroll: true });
    }
  }

  function pick(i: number) {
    if (phase !== 'menu') return;
    const a = items[i];
    if (!a || a.kind === 'off') return;
    picked = a;
    active = null;
    tried = true;
    if (a.kind === 'tool') {
      phase = 'tool';
      announce = `${a.label}: in Morph, a window opens to ${a.result}.`;
      later(2600, finish);
      return;
    }
    phase = 'working';
    announce = 'Working…';
    later(reduced ? 300 : 900, () => {
      phase = 'done';
      announce = `Saved ${a.result}, next to the original.`;
      later(2400, finish);
    });
  }

  function finish() {
    const wasFocusInside = orbEl.contains(document.activeElement);
    const id = consumed;
    reset();
    if (wasFocusInside && id) stage.querySelector<HTMLButtonElement>(`[data-file="${id}"]`)?.focus();
  }

  // Sends a card flying from where it is now into the orb's centre, then opens the orb.
  function flyIn(f: SampleFile, from: { x: number; y: number }, viaKeyboard: boolean) {
    if (reduced) {
      open(f, viaKeyboard);
      return;
    }
    const card = stage.querySelector<HTMLElement>(`[data-file="${f.id}"]`)!;
    const c = card.getBoundingClientRect();
    const o = orbEl.getBoundingClientRect();
    flying = {
      id: f.id,
      x: o.left + o.width / 2 - (c.left - from.x + c.width / 2),
      y: o.top + o.height / 2 - (c.top - from.y + c.height / 2),
    };
    later(260, () => open(f, viaKeyboard));
  }

  // Pointer dragging (mouse, pen and touch).
  let start: { x: number; y: number; id: number } | null = null;
  let moved = false;

  function onDown(e: PointerEvent, f: SampleFile) {
    if (phase === 'working' || e.button > 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    start = { x: e.clientX, y: e.clientY, id: e.pointerId };
    moved = false;
    dragId = f.id;
    dragX = dragY = 0;
  }

  function onMove(e: PointerEvent) {
    if (!start || e.pointerId !== start.id) return;
    const dx = e.clientX - start.x;
    const dy = e.clientY - start.y;
    if (!moved && Math.hypot(dx, dy) < 6) return;
    moved = true;
    dragX = dx;
    dragY = dy;
    const o = orbEl.getBoundingClientRect();
    const r = (o.width / 2) * (BACKDROP_R / C);
    over = Math.hypot(e.clientX - (o.left + o.width / 2), e.clientY - (o.top + o.height / 2)) < r;
  }

  function onUp(e: PointerEvent, f: SampleFile) {
    if (!start || e.pointerId !== start.id) return;
    start = null;
    const wasOver = over;
    over = false;
    const at = { x: dragX, y: dragY };
    dragId = null;
    if (!moved) return; // a plain click is handled by onclick
    if (wasOver) {
      dragX = at.x;
      dragY = at.y;
      flyIn(f, at, false);
    }
    dragX = dragY = 0;
  }

  function onClick(e: MouseEvent, f: SampleFile) {
    if (moved) {
      moved = false;
      return;
    }
    if (phase === 'working') return;
    if (consumed === f.id) {
      reset();
      return;
    }
    reset();
    flyIn(f, { x: 0, y: 0 }, e.detail === 0);
  }

  function onRingKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      finish();
      return;
    }
    if (phase !== 'menu') return;
    const n = items.length;
    const current = Number((document.activeElement as HTMLElement)?.dataset?.index ?? -1);
    let next: number | null = null;
    if (/^[1-9]$/.test(e.key) && Number(e.key) <= n) {
      e.preventDefault();
      pick(Number(e.key) - 1);
      return;
    }
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (current + 1 + n) % n;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (current - 1 + n) % n;
    if (e.key === 'Home') next = 0;
    if (e.key === 'End') next = n - 1;
    if ((e.key === 'Enter' || e.key === ' ') && current >= 0) {
      e.preventDefault();
      pick(current);
      return;
    }
    if (next !== null) {
      e.preventDefault();
      ringEl?.querySelector<SVGGElement>(`[data-index="${next}"]`)?.focus();
    }
  }

  onMount(() => {
    reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
    // The floating cards stop when the orb is off screen.
    const io = new IntersectionObserver(([e]) => (visible = e.isIntersecting));
    io.observe(stage);
    return () => {
      io.disconnect();
      clearTimers();
    };
  });

  function sliceFill(i: number, a: Action) {
    const on = active === i;
    if (a.kind === 'off') return on ? hsl(file!.category.hue, 18, 30) : hsl(file!.category.hue, 10, 26, 0.5);
    return on ? hsl(file!.category.hue, 78, 58) : hsl(file!.category.hue, 55, 26, 0.92);
  }
</script>

<div class="stage" class:paused={!visible} class:hot={over || phase !== 'idle'} bind:this={stage}>
  <div class="glow" aria-hidden="true"></div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="orb" class:over bind:this={orbEl} onkeydown={onRingKey}>
    <svg viewBox="0 0 {SIZE} {SIZE}" role="group" aria-label="Morph's orb (demo)">
      <defs>
        <radialGradient id="hub-fill" cx="50%" cy="35%" r="70%">
          <stop offset="0" stop-color="#2c2935" />
          <stop offset="1" stop-color="#1a1820" />
        </radialGradient>
      </defs>

      <circle class="disc" cx={C} cy={C} r={BACKDROP_R} />
      <circle class="drop-ring" cx={C} cy={C} r={BACKDROP_R} />

      {#if phase === 'idle' && !over}
        <g class="ghost" aria-hidden="true">
          {#each ghost as s, i}
            <path
              d={annular(R0, R1, s.start, s.end)}
              fill={hsl(GHOST_HUES[i], 45, 24, 0.55)}
              stroke={hsl(GHOST_HUES[i], 70, 70, 0.14)}
            />
          {/each}
        </g>
      {/if}

      {#if phase === 'menu' && !over}
        {#key openCount}
          <g class="ring" bind:this={ringEl}>
            {#each items as a, i}
              {@const s = slices[i]}
              {@const [lx, ly] = polar(LABEL_R, s.mid)}
              {@const [kx, ky] = polar(R1 - 13, s.mid)}
              <g
                class="slice"
                class:on={active === i}
                class:off={a.kind === 'off'}
                style="--i:{i}"
                data-slice
                data-index={i}
                role="button"
                tabindex={i === 0 ? 0 : -1}
                aria-label="{a.label}: {a.hint}"
                aria-disabled={a.kind === 'off'}
                onpointerenter={() => (active = i)}
                onpointerleave={() => (active = null)}
                onfocus={() => (active = i)}
                onblur={() => (active = null)}
                onclick={() => pick(i)}
              >
                <path
                  d={annular(R0, R1, s.start, s.end)}
                  fill={sliceFill(i, a)}
                  stroke={hsl(file!.category.hue, 70, 70, 0.18)}
                />
                <text x={lx} y={ly} class="label">{a.label}</text>
                {#if i < 9}<text x={kx} y={ky} class="key">{i + 1}</text>{/if}
              </g>
            {/each}
          </g>
        {/key}
      {/if}

      {#if phase === 'working'}
        <circle class="spinner" cx={C} cy={C} r={HUB + 7} />
      {/if}

      <g
        class="hub"
        class:back={phase === 'menu'}
        role={phase === 'menu' ? 'button' : undefined}
        tabindex={phase === 'menu' ? 0 : undefined}
        aria-label={phase === 'menu' ? 'Back: take the file out' : undefined}
        onclick={() => phase === 'menu' && finish()}
        onkeydown={(e) => phase === 'menu' && (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), finish())}
      >
        <circle cx={C} cy={C} r={HUB} fill="url(#hub-fill)" stroke="rgb(255 255 255 / 0.1)" />
        {#if phase === 'done'}
          <path class="check" d="M{C - 9} {C - 9} l6 6 l12 -13" pathLength="1" />
          <text x={C} y={C + 15} class="sub">Saved</text>
        {:else}
          {#if phase === 'menu'}<text x={C} y={C - 21} class="back-label">‹ back</text>{/if}
          <text x={C} y={phase === 'menu' ? C - 3 : C - 6} class="title">{title}</text>
          <foreignObject x={C - 46} y={phase === 'menu' ? C + 6 : C + 3} width="92" height="34">
            <p class="hint" xmlns="http://www.w3.org/1999/xhtml">{subtitle}</p>
          </foreignObject>
        {/if}
      </g>
    </svg>
  </div>

  <ul class="files" aria-label="Sample files">
    {#each FILES as f, i}
      <li class="float f{i}">
        <button
          type="button"
          class="file"
          class:dragging={dragId === f.id && (dragX !== 0 || dragY !== 0)}
          class:gone={consumed === f.id && !flying}
          class:flying={flying?.id === f.id}
          data-file={f.id}
          style:--x="{flying?.id === f.id ? flying.x : dragId === f.id ? dragX : 0}px"
          style:--y="{flying?.id === f.id ? flying.y : dragId === f.id ? dragY : 0}px"
          style:--hue={f.category.hue}
          aria-label="Put {f.name} in the orb"
          onpointerdown={(e) => onDown(e, f)}
          onpointermove={onMove}
          onpointerup={(e) => onUp(e, f)}
          onpointercancel={(e) => onUp(e, f)}
          onclick={(e) => onClick(e, f)}
        >
          <span class="icon" aria-hidden="true">
            <svg viewBox="0 0 24 24"><path d="M6 2.75h8.5L19.25 7.5v13.75H6z" /><path d="M14.25 2.75V7.75h5" /></svg>
            <b>{f.name.split('.').pop()?.toUpperCase()}</b>
          </span>
          <span class="meta">
            <span class="name">{f.name}</span>
            <span class="size">{f.size}</span>
          </span>
        </button>
      </li>
    {/each}
  </ul>

  <div class="caption" aria-hidden="true">
    {#if phase === 'done' && picked && file}
      <p class="result">
        <svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
        <span><b>{picked.result}</b> saved next to {file.name}</span>
      </p>
    {:else if tried}
      <p>That was a demo. Morph does it for real, on your computer.</p>
    {:else}
      <p>Drag a file onto the orb, or tap one.</p>
    {/if}
  </div>
  <p class="sr-only" aria-live="polite">{announce}</p>
</div>

<style>
  .stage {
    position: relative;
    width: 100%;
    max-width: 540px;
    margin-inline: auto;
    display: grid;
    justify-items: center;
    gap: 20px;
    user-select: none;
    -webkit-user-select: none;
  }

  .glow {
    position: absolute;
    left: 50%;
    top: 38%;
    width: 120%;
    aspect-ratio: 1;
    translate: -50% -50%;
    background: radial-gradient(closest-side, rgb(124 92 255 / 0.32), rgb(124 92 255 / 0.1) 55%, transparent);
    opacity: 0.75;
    scale: 0.92;
    pointer-events: none;
    transition:
      opacity var(--dur-slow) var(--ease-out),
      scale var(--dur-slow) var(--ease-out);
  }

  .hot .glow {
    opacity: 1;
    scale: 1.04;
  }

  .orb {
    position: relative;
    width: min(100%, 380px);
    aspect-ratio: 1;
    transition: scale var(--dur) var(--ease-spring);
  }

  .orb.over {
    scale: 1.035;
  }

  svg {
    width: 100%;
    height: 100%;
    overflow: visible;
    font-family: 'Segoe UI', var(--font);
  }

  .disc {
    fill: rgb(24 22 30 / 0.97);
    stroke: rgb(255 255 255 / 0.1);
    stroke-width: 1;
    filter: drop-shadow(0 24px 48px rgb(0 0 0 / 0.5));
  }

  .drop-ring {
    fill: none;
    stroke: var(--accent);
    stroke-width: 4;
    stroke-dasharray: 10 8;
    opacity: 0;
    transition: opacity var(--dur) var(--ease-out);
  }

  .over .drop-ring {
    opacity: 1;
    animation: dash 1.2s linear infinite;
  }

  @keyframes dash {
    to {
      stroke-dashoffset: -18;
    }
  }

  .ghost {
    animation: fade-in var(--dur-slow) var(--ease-out);
  }

  .ring {
    transform-box: view-box;
    transform-origin: 50% 50%;
    animation: ring-in 420ms var(--ease-out) both;
  }

  @keyframes ring-in {
    from {
      opacity: 0;
      transform: scale(0.72);
    }
  }

  .slice {
    cursor: pointer;
    outline: none;
    transform-box: view-box;
    transform-origin: 50% 50%;
    transition: transform 220ms var(--ease-out);
    animation: slice-in 360ms var(--ease-out) both;
    animation-delay: calc(60ms + var(--i) * 28ms);
  }

  @keyframes slice-in {
    from {
      opacity: 0;
      transform: scale(0.94);
    }
  }

  .slice path {
    transition: fill 200ms var(--ease-out);
  }

  .slice.on {
    transform: scale(1.035);
  }

  .slice.off {
    cursor: not-allowed;
  }

  .slice:focus-visible path {
    stroke: #fff;
    stroke-width: 2;
  }

  text {
    text-anchor: middle;
    dominant-baseline: central;
    pointer-events: none;
  }

  .label {
    font-size: 13px;
    font-weight: 600;
    fill: #f1eff6;
    transition: fill 200ms var(--ease-out);
  }

  .on .label {
    fill: #fff;
  }

  .off .label {
    fill: #a19dad;
  }

  .key {
    font-size: 9px;
    fill: #a19dad;
  }

  .on:not(.off) .key {
    fill: rgb(255 255 255 / 0.8);
  }

  .hub {
    outline: none;
  }

  .hub.back {
    cursor: pointer;
  }

  .hub.back:hover circle,
  .hub.back:focus-visible circle {
    stroke: rgb(155 130 255 / 0.6);
  }

  .title {
    font-size: 15px;
    font-weight: 700;
    fill: #f1eff6;
  }

  .back-label {
    font-size: 10px;
    font-weight: 600;
    fill: #9b82ff;
  }

  .hint {
    margin: 0;
    font-size: 10px;
    line-height: 1.2;
    color: #a19dad;
    text-align: center;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .sub {
    font-size: 10px;
    fill: #a19dad;
  }

  .check {
    fill: none;
    stroke: #9b82ff;
    stroke-width: 3;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-dasharray: 1;
    stroke-dashoffset: 1;
    animation: draw 420ms var(--ease-out) 60ms forwards;
  }

  @keyframes draw {
    to {
      stroke-dashoffset: 0;
    }
  }

  .spinner {
    fill: none;
    stroke: var(--accent);
    stroke-width: 3;
    stroke-linecap: round;
    stroke-dasharray: 90 400;
    transform-box: view-box;
    transform-origin: 50% 50%;
    animation:
      spin 0.9s linear infinite,
      fade-in var(--dur) var(--ease-out);
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes fade-in {
    from {
      opacity: 0;
    }
  }

  /* Sample files */
  .files {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .file {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 52px;
    padding: 8px 14px 8px 10px;
    border-radius: 14px;
    border: 1px solid var(--border-strong);
    background: rgb(29 26 37 / 0.92);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: grab;
    touch-action: none;
    box-shadow: 0 10px 30px -12px rgb(0 0 0 / 0.6);
    transform: translate(var(--x), var(--y));
    transition:
      transform 480ms var(--ease-spring),
      opacity var(--dur) var(--ease-out),
      border-color var(--dur) var(--ease-out),
      box-shadow var(--dur) var(--ease-out);
  }

  .file:hover {
    border-color: hsl(var(--hue) 60% 60% / 0.55);
  }

  .file.dragging {
    cursor: grabbing;
    transition: none;
    transform: translate(var(--x), var(--y)) scale(1.05) rotate(-2deg);
    box-shadow: 0 24px 50px -14px rgb(0 0 0 / 0.75);
    z-index: 5;
    position: relative;
  }

  .file.flying {
    transform: translate(var(--x), var(--y)) scale(0.3);
    opacity: 0;
    transition:
      transform 300ms var(--ease-in),
      opacity 300ms var(--ease-in);
    position: relative;
    z-index: 5;
  }

  /* Back at its spot (no flight back), faded while its copy is in the orb. */
  .file.gone {
    opacity: 0.28;
    transform: scale(0.94);
    transition: opacity 400ms var(--ease-out);
  }

  .icon {
    position: relative;
    display: grid;
    place-items: center;
    width: 30px;
    height: 34px;
    flex: none;
  }

  .icon svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    fill: hsl(var(--hue) 45% 22%);
    stroke: hsl(var(--hue) 70% 70%);
    stroke-width: 1.3;
    stroke-linejoin: round;
  }

  .icon b {
    position: relative;
    margin-top: 9px;
    font-size: 6.5px;
    font-weight: 700;
    letter-spacing: 0;
    color: hsl(var(--hue) 85% 82%);
  }

  .meta {
    display: grid;
    line-height: 1.25;
  }

  .name {
    font-size: 14px;
    font-weight: 600;
  }

  .size {
    font-size: 12.5px;
    color: var(--muted);
  }

  .caption {
    display: grid;
    place-items: center;
    min-height: 36px;
    color: var(--muted);
    font-size: 14px;
    text-align: center;
  }

  .caption p {
    margin: 0;
    animation: caption-in 360ms var(--ease-out);
  }

  .result {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 14px 6px 10px;
    border-radius: 999px;
    background: rgb(124 92 255 / 0.14);
    border: 1px solid rgb(155 130 255 / 0.35);
    color: var(--text);
  }

  .result svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: var(--accent);
    stroke-width: 2.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .result b {
    font-weight: 600;
  }

  @keyframes caption-in {
    from {
      opacity: 0;
      translate: 0 6px;
    }
  }

  /* Wide screens: the files float around the orb instead of sitting in a row. */
  @media (min-width: 1000px) {
    .stage {
      max-width: 580px;
      aspect-ratio: 1 / 1.02;
      align-content: center;
    }

    .orb {
      width: 76%;
    }

    .files {
      position: absolute;
      inset: 0;
      display: block;
      pointer-events: none;
    }

    .float {
      position: absolute;
      pointer-events: auto;
      animation: float 6s ease-in-out infinite;
    }

    .f0 {
      left: -4%;
      top: 6%;
    }

    .f1 {
      right: -6%;
      top: 44%;
      animation-delay: -2s;
    }

    .f2 {
      left: 2%;
      bottom: 13%;
      animation-delay: -4s;
    }

    .paused .float {
      animation-play-state: paused;
    }

    .caption {
      position: absolute;
      bottom: 0;
      left: 0;
      right: 0;
    }
  }

  @keyframes float {
    0%,
    100% {
      translate: 0 0;
    }
    50% {
      translate: 0 -7px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .float {
      animation: none !important;
    }
  }
</style>
