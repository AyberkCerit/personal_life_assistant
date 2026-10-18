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
