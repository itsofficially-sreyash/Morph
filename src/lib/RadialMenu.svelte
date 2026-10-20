<script lang="ts">
  import { annularPath, polar, sectors, type RingItem } from "$lib/radial";

  interface Props {
    items: RingItem[];
    /** Main text in the hub. */
    title: string;
    /** Smaller line under the title. */
    subtitle: string;
    /** Shows a back arrow in the hub and makes it clickable. */
    canGoBack: boolean;
    /** Bumped by the parent to replay the opening animation. */
    animationKey: number;
    /** Highlights the orb, e.g. while files are dragged over it. */
    glow?: boolean;
    onpick: (item: RingItem) => void;
    onback: () => void;
  }

  let { items, title, subtitle, canGoBack, animationKey, glow = false, onpick, onback }: Props = $props();

  const SIZE = 380;
  const C = SIZE / 2;
  const HUB = 56;
  const R0 = 66;
  const R1 = 170;
  const LABEL_R = (R0 + R1) / 2;
  // Text box inside the hub, kept clear of the circle's curved edge.
  const HUB_TEXT_W = 92;
  const HUB_TEXT_H = 88;

  let active = $state(-1);
  const slices = $derived(sectors(items.length));
  const hovered = $derived(active >= 0 ? items[active] : undefined);

  // A new ring (category → actions, or a fresh show) starts with nothing highlighted.
  $effect(() => {
    void items;
    void animationKey;
    active = -1;
  });

  export function handleKey(e: KeyboardEvent): boolean {
    const n = items.length;
    if (/^[1-9]$/.test(e.key) && Number(e.key) <= n) {
      onpick(items[Number(e.key) - 1]);
    } else if (e.key === "ArrowRight" || e.key === "ArrowDown" || (e.key === "Tab" && !e.shiftKey)) {
      active = n ? (active + 1 + n) % n : -1;
    } else if (e.key === "ArrowLeft" || e.key === "ArrowUp" || (e.key === "Tab" && e.shiftKey)) {
      active = n ? (active - 1 + n) % n : -1;
    } else if (e.key === "Enter" && active >= 0) {
      onpick(items[active]);
    } else if (e.key === "Backspace" && canGoBack) {
      onback();
    } else {
      return false;
    }
    return true;
  }
</script>

{#key animationKey}
  <!-- Scales to the window instead of assuming 380 CSS px: Windows "Text size" (and zoom)
       shrink the page's CSS viewport, which used to clip the ring's right and bottom edges. -->
  <svg
    class="ring"
    viewBox="0 0 {SIZE} {SIZE}"
    width="100%"
    height="100%"
    preserveAspectRatio="xMidYMid meet"
    role="menu"
    aria-label={title}
  >
    <defs>
      <radialGradient id="hub-fill" cx="50%" cy="35%" r="70%">
        <stop offset="0%" stop-color="var(--hub-hi)" />
        <stop offset="100%" stop-color="var(--hub-lo)" />
      </radialGradient>
    </defs>

    <circle class="backdrop" class:glow cx={C} cy={C} r={R1 + 6} />

    {#each items as item, i (item.id + i)}
      {@const s = slices[i]}
      {@const [lx, ly] = polar(C, C, LABEL_R, s.mid)}
      <g
        class="slice"
        class:active={i === active}
        style="--hue: {item.hue}; --delay: {i * 22}ms"
        role="menuitem"
        tabindex="-1"
        aria-label={item.hint}
        onpointerenter={() => (active = i)}
        onpointerleave={() => (active = active === i ? -1 : active)}
        onclick={() => onpick(item)}
        onkeydown={() => {}}
      >
        <path d={annularPath(C, C, R0, R1, s)} />
        <text x={lx} y={ly} class="label">{item.label}</text>
        {#if i < 9}
          {@const [kx, ky] = polar(C, C, R1 - 13, s.mid)}
          <text x={kx} y={ky} class="key">{i + 1}</text>
        {/if}
      </g>
    {/each}

    <g
      class="hub"
      class:clickable={canGoBack}
      role="button"
      tabindex="-1"
      aria-label={canGoBack ? "Back" : title}
      onclick={() => canGoBack && onback()}
      onkeydown={() => {}}
    >
      <circle cx={C} cy={C} r={HUB} />
      <!-- SVG text can't wrap, so the hub's text lives in an HTML box that fits inside the circle. -->
      <foreignObject x={C - HUB_TEXT_W / 2} y={C - HUB_TEXT_H / 2} width={HUB_TEXT_W} height={HUB_TEXT_H}>
        <div class="hub-text">
          {#if canGoBack}
            <span class="back">‹ back</span>
          {/if}
          <span class="title">{title}</span>
          <span class="subtitle">{hovered ? hovered.hint : subtitle}</span>
        </div>
      </foreignObject>
    </g>
  </svg>
{/key}

<style>
  .ring {
    display: block;
    overflow: visible;
    font-family: "Segoe UI", system-ui, Ubuntu, Cantarell, sans-serif;
    user-select: none;
    animation: open 180ms cubic-bezier(0.2, 0.9, 0.3, 1.2) both;
  }
  @keyframes open {
    from {
      transform: scale(0.72);
      opacity: 0;
    }
  }
  .backdrop {
    fill: var(--ring-bg);
    stroke: var(--ring-edge);
    stroke-width: 1;
    transition:
      stroke 120ms,
      stroke-width 120ms;
  }
  .backdrop.glow {
    stroke: var(--accent);
    stroke-width: 4;
    stroke-dasharray: 10 8;
    animation: spin 6s linear infinite;
    transform-origin: 190px 190px;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .slice {
    cursor: pointer;
    transform-origin: 190px 190px;
    animation: slice-in 220ms ease-out both;
    animation-delay: var(--delay);
    outline: none;
  }
  @keyframes slice-in {
    from {
      opacity: 0;
      transform: scale(0.86) rotate(-8deg);
    }
  }
  .slice path {
    fill: hsl(var(--hue) 55% var(--slice-l) / var(--slice-a));
    stroke: hsl(var(--hue) 70% 70% / 0.18);
    stroke-width: 1;
    transition:
      fill 120ms,
      transform 140ms;
    transform-origin: 190px 190px;
  }
  .slice.active path {
    fill: hsl(var(--hue) 78% 58%);
    transform: scale(1.035);
  }
  .label {
    fill: var(--text);
    font-size: 13px;
    font-weight: 600;
    text-anchor: middle;
    dominant-baseline: central;
    pointer-events: none;
  }
  .slice.active .label {
    fill: #fff;
  }
  .key {
    fill: var(--muted);
    font-size: 9px;
    text-anchor: middle;
    dominant-baseline: central;
    pointer-events: none;
  }
  .slice.active .key {
    fill: rgba(255, 255, 255, 0.8);
  }
  .hub circle {
    fill: url(#hub-fill);
    stroke: var(--ring-edge);
  }
  .hub.clickable {
    cursor: pointer;
  }
  .hub.clickable:hover circle {
    stroke: var(--accent);
  }
  .hub foreignObject {
    pointer-events: none;
  }
  .hub-text {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    text-align: center;
    line-height: 1.2;
  }
  .title {
    color: var(--text);
    font-size: 15px;
    font-weight: 700;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .subtitle {
    color: var(--muted);
    font-size: 10px;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
  }
  .back {
    color: var(--accent);
    font-size: 10px;
    font-weight: 600;
  }
</style>
