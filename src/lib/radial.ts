// Geometry for the radial menu. Angles are in degrees, 0° = 12 o'clock, clockwise.

/** One slice of the ring. */
export interface RingItem {
  id: string;
  label: string;
  hint: string;
  hue: number;
}

export interface Sector {
  start: number;
  end: number;
  /** Middle of the sector, used to place its label. */
  mid: number;
}

/** Splits the circle into `count` equal sectors, the first one centred at 12 o'clock. */
export function sectors(count: number, gapDeg = 2): Sector[] {
  if (count <= 0) return [];
  const span = 360 / count;
  return Array.from({ length: count }, (_, i) => {
    const mid = i * span;
    const half = (span - (count > 1 ? gapDeg : 0)) / 2;
    return { start: mid - half, end: mid + half, mid };
  });
}

export function polar(cx: number, cy: number, r: number, deg: number): [number, number] {
  const rad = ((deg - 90) * Math.PI) / 180;
  return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)];
}

/** SVG path for an annular (donut) slice between radii `r0` and `r1`. */
export function annularPath(cx: number, cy: number, r0: number, r1: number, s: Sector): string {
  // A full ring can't be drawn as one arc, so split it into two halves.
  if (s.end - s.start >= 359.99) {
    const half = { start: s.start, end: s.start + 180, mid: s.start + 90 };
    const rest = { start: s.start + 180, end: s.start + 360, mid: s.start + 270 };
    return annularPath(cx, cy, r0, r1, half) + annularPath(cx, cy, r0, r1, rest);
  }
  const large = s.end - s.start > 180 ? 1 : 0;
  const [ox0, oy0] = polar(cx, cy, r1, s.start);
  const [ox1, oy1] = polar(cx, cy, r1, s.end);
  const [ix1, iy1] = polar(cx, cy, r0, s.end);
  const [ix0, iy0] = polar(cx, cy, r0, s.start);
  return [
    `M ${ox0} ${oy0}`,
    `A ${r1} ${r1} 0 ${large} 1 ${ox1} ${oy1}`,
    `L ${ix1} ${iy1}`,
    `A ${r0} ${r0} 0 ${large} 0 ${ix0} ${iy0}`,
    "Z",
  ].join(" ");
}
