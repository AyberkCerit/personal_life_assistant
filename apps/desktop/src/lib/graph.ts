// Pure parts of the graph view: how big a note's dot is and which dot a point hits.

export interface Pt { x: number; y: number; r: number }

/** How many links each of the `count` notes has. */
export function degrees(count: number, edges: readonly (readonly [number, number])[]): number[] {
  const out = new Array<number>(count).fill(0);
  for (const [a, b] of edges) {
    out[a]++;
    out[b]++;
  }
  return out;
}

/** A note with more links gets a bigger dot, growing slowly so a hub does not swallow the rest. */
export function radius(degree: number): number {
  return 3 + Math.sqrt(degree) * 1.6;
}

/** The dot under (x, y), the nearest when several overlap; `slack` widens small dots to a fingertip. */
export function nearest(points: readonly Pt[], x: number, y: number, slack = 0): number {
  let best = -1;
  let bestDistance = Infinity;
  points.forEach((p, i) => {
    const d = Math.hypot(p.x - x, p.y - y);
    if (d <= p.r + slack && d < bestDistance) {
      best = i;
      bestDistance = d;
    }
  });
  return best;
}

/** For each note, the notes it is linked to. */
export function neighbours(count: number, edges: readonly (readonly [number, number])[]): Set<number>[] {
  const out = Array.from({ length: count }, () => new Set<number>());
  for (const [a, b] of edges) {
    out[a].add(b);
    out[b].add(a);
  }
  return out;
}

/** A label's box on the canvas: left, top, width, height. */
export type Box = readonly [number, number, number, number];

/** Whether two label boxes cover each other. */
export function overlaps(a: Box, b: Box): boolean {
  return a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
}

/** A title cut to `max` characters for the graph, so one long name does not cover its neighbours. */
export function shortLabel(title: string, max = 28): string {
  const chars = [...title];
  return chars.length <= max ? title : `${chars.slice(0, max - 1).join("").trimEnd()}…`;
}
