/** Durations for Svelte transitions, matching --duration-fast / --duration-base (spec § 3.3). */
export const FAST = 120;
export const BASE = 200;

/** `ms`, or 0 when the user asked the OS for reduced motion. */
export function motion(ms: number): number {
  const reduce = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  return reduce ? 0 : ms;
}
