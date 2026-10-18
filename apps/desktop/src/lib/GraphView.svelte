<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY, type Simulation, type SimulationNodeDatum } from "d3-force";
  import Network from "@lucide/svelte/icons/network";
  import { api, type NoteGraph } from "./api";
  import { degrees, nearest, neighbours, radius } from "./graph";
  import { t, tf } from "./i18n";
  import EmptyState from "./ui/EmptyState.svelte";

  // The middle pane's graph of the vault: a dot per note, a line per link (like Obsidian's graph view).
  // Drawn on a canvas and laid out by d3-force; it only runs while the view is open and rests once the
  // dots settle.
  let { current, onOpen }: { current: string | null; onOpen: (path: string) => void } = $props();

  interface Dot extends SimulationNodeDatum { x: number; y: number; r: number; title: string; path: string }

  let graph = $state.raw<NoteGraph | null>(null);
  let failed = $state(false);
  let canvas = $state<HTMLCanvasElement>();
  let wrap = $state<HTMLDivElement>();

  async function load() {
    try {
      graph = await api.noteGraph();
      failed = false;
    } catch {
      graph = null; // no vault open (yet): the graph comes with the next change
      failed = true;
    }
  }

  onMount(() => {
    void load();
    const un = api.onIndexChanged(() => void load()); // a note added or relinked while the graph is open
    return () => void un.then((f) => f());
  });

  // What is drawn. Plain variables: the canvas is painted by hand, Svelte does not render it.
  let dots: Dot[] = [];
  let edges: [number, number][] = [];
  let near: Set<number>[] = [];
  let sim: Simulation<Dot, undefined> | null = null;
  let hover = -1;
  let view = { k: 1, x: 0, y: 0 };
  let touched = false; // the user moved the view: do not re-fit it under them
  let size = { w: 0, h: 0, dpr: 1 };
  const seen = new Map<string, { x: number; y: number }>(); // where notes were, so a reload keeps the layout
  const reduceMotion = () => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  function fit() {
    if (!dots.length || !size.w) return;
    const xs = dots.map((d) => d.x);
    const ys = dots.map((d) => d.y);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const pad = 48;
    const k = Math.min(2, Math.max(0.1, Math.min((size.w - 2 * pad) / Math.max(x1 - x0, 1), (size.h - 2 * pad) / Math.max(y1 - y0, 1))));
    view = { k, x: size.w / 2 - ((x0 + x1) / 2) * k, y: size.h / 2 - ((y0 + y1) / 2) * k };
  }

  function draw() {
    const c = canvas;
    const ctx = c?.getContext("2d");
    if (!c || !ctx || !size.w) return;
    const css = getComputedStyle(c);
    const color = (name: string) => css.getPropertyValue(name).trim() || "currentColor";
    ctx.setTransform(size.dpr, 0, 0, size.dpr, 0, 0);
    ctx.clearRect(0, 0, size.w, size.h);
    ctx.translate(view.x, view.y);
    ctx.scale(view.k, view.k);
    const focus = hover >= 0;
    const related = (i: number) => i === hover || near[hover]?.has(i);
    ctx.lineWidth = 1 / view.k;
    for (const lit of [false, true]) {
      ctx.globalAlpha = focus && !lit ? 0.18 : 0.7;
      ctx.strokeStyle = lit ? color("--color-accent") : color("--color-border-strong");
      ctx.beginPath();
      for (const [a, b] of edges) {
        if (focus && (a === hover || b === hover) !== lit) continue;
        ctx.moveTo(dots[a].x, dots[a].y);
        ctx.lineTo(dots[b].x, dots[b].y);
      }
      ctx.stroke();
    }
    const open = dots.findIndex((d) => d.path === current);
    dots.forEach((d, i) => {
      ctx.globalAlpha = focus && !related(i) ? 0.25 : 1;
      ctx.fillStyle = i === open || i === hover ? color("--color-accent") : color("--color-text-muted");
      ctx.beginPath();
      ctx.arc(d.x, d.y, d.r, 0, Math.PI * 2);
      ctx.fill();
    });
    ctx.globalAlpha = 1;
    ctx.fillStyle = color("--color-text");
    ctx.font = `${12 / view.k}px ${css.fontFamily}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    dots.forEach((d, i) => {
      if (view.k >= 1.8 || i === open || (focus && related(i))) ctx.fillText(d.title, d.x, d.y + d.r + 3 / view.k);
    });
  }

  function resize() {
    if (!wrap || !canvas) return;
    const w = wrap.clientWidth;
    const h = wrap.clientHeight;
    const dpr = window.devicePixelRatio || 1;
    view.x += (w - size.w) / 2; // the picture stays centred when the pane grows or shrinks
    view.y += (h - size.h) / 2;
    size = { w, h, dpr };
    canvas.width = Math.max(1, Math.round(w * dpr));
    canvas.height = Math.max(1, Math.round(h * dpr));
    if (!touched) fit();
    draw();
  }

  function start(g: NoteGraph, c: HTMLCanvasElement, box: HTMLDivElement) {
    const deg = degrees(g.nodes.length, g.edges);
    dots = g.nodes.map((n, i) => {
      const old = seen.get(n.path);
      const spiral = 10 * Math.sqrt(i + 0.5); // d3's own starting spiral, so new notes do not begin stacked
      return { x: old?.x ?? spiral * Math.cos(i * 2.399963), y: old?.y ?? spiral * Math.sin(i * 2.399963), r: radius(deg[i]), title: n.title, path: n.path };
    });
    edges = g.edges;
    near = neighbours(dots.length, edges);
    hover = -1;
    const kept = seen.size > 0 && dots.some((d) => seen.has(d.path));
    touched = touched && kept;
    sim = forceSimulation(dots)
      .force("link", forceLink<Dot, { source: number; target: number }>(edges.map(([source, target]) => ({ source, target }))).distance(32).strength(0.5))
      .force("charge", forceManyBody<Dot>().strength(-50).distanceMax(320))
      .force("x", forceX<Dot>(0).strength(0.04))
      .force("y", forceY<Dot>(0).strength(0.04))
      .force("collide", forceCollide<Dot>().radius((d) => d.r + 2))
      .alpha(kept ? 0.3 : 1)
      .on("tick", draw)
      .on("end", () => {
        for (const d of dots) seen.set(d.path, { x: d.x, y: d.y });
        if (!touched) fit();
        draw();
      });
    if (reduceMotion()) {
      sim.stop();
      sim.tick(300);
      for (const d of dots) seen.set(d.path, { x: d.x, y: d.y });
      if (!touched) fit();
    }
    resize();
    const sized = new ResizeObserver(resize);
    sized.observe(box);
    const themed = new MutationObserver(draw); // a theme switch repaints in the new colours
    themed.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const at = c.getBoundingClientRect();
      const [px, py] = [e.clientX - at.left, e.clientY - at.top];
      const k = Math.min(8, Math.max(0.1, view.k * Math.exp(-e.deltaY * 0.0015)));
      view = { k, x: px - ((px - view.x) / view.k) * k, y: py - ((py - view.y) / view.k) * k };
      touched = true;
      draw();
    };
    c.addEventListener("wheel", wheel, { passive: false });
    return () => {
      sim?.stop();
      sim = null;
      sized.disconnect();
      themed.disconnect();
      c.removeEventListener("wheel", wheel);
    };
  }

  $effect(() => {
    const [g, c, box] = [graph, canvas, wrap];
    if (!g || !c || !box || !g.edges.length) return;
    return untrack(() => start(g, c, box));
  });

  $effect(() => {
    void current;
    untrack(draw);
  });

  // ---- pointer: drag a dot, pan the background, click a dot to open its note
  let grab: { i: number; sx: number; sy: number; moved: boolean; panX: number; panY: number } | null = null;

  const world = (e: PointerEvent) => {
    const at = canvas!.getBoundingClientRect();
    return { x: (e.clientX - at.left - view.x) / view.k, y: (e.clientY - at.top - view.y) / view.k };
  };
  const hit = (e: PointerEvent) => {
    const p = world(e);
    return nearest(dots, p.x, p.y, 4 / view.k);
  };

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    canvas!.setPointerCapture(e.pointerId);
    grab = { i: hit(e), sx: e.clientX, sy: e.clientY, moved: false, panX: view.x, panY: view.y };
  }

  function move(e: PointerEvent) {
    if (!grab) {
      const i = hit(e);
      if (i !== hover) {
        hover = i;
        canvas!.style.cursor = i >= 0 ? "pointer" : "grab";
        draw();
      }
      return;
    }
    const [dx, dy] = [e.clientX - grab.sx, e.clientY - grab.sy];
    if (!grab.moved && Math.hypot(dx, dy) < 4) return;
    grab.moved = true;
    if (grab.i >= 0) {
      const p = world(e);
      dots[grab.i].fx = p.x;
      dots[grab.i].fy = p.y;
      hover = grab.i;
      sim?.alphaTarget(0.3).restart();
    } else {
      touched = true;
      view = { ...view, x: grab.panX + dx, y: grab.panY + dy };
      draw();
    }
  }

  function up() {
    if (!grab) return;
    const { i, moved } = grab;
    grab = null;
    if (i < 0) return;
    if (moved) {
      dots[i].fx = null;
      dots[i].fy = null;
      sim?.alphaTarget(0);
    } else {
      onOpen(dots[i].path);
    }
  }

  function leave() {
    if (grab || hover < 0) return;
    hover = -1;
    draw();
  }

  const label = $derived(tf("graph.count", { notes: graph?.nodes.length ?? 0, links: graph?.edges.length ?? 0 }));
</script>

{#if graph && graph.edges.length}
  <div class="graph" bind:this={wrap}>
    <canvas bind:this={canvas} aria-label="{t('graph.label')} ({label})" onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onpointerleave={leave}></canvas>
    <p class="hint">{t("graph.hint")} · {label}</p>
  </div>
{:else if graph || failed}
  <EmptyState icon={Network} title={t("graph.empty")} />
{:else}
  <p class="loading">{t("graph.loading")}</p>
{/if}

<style>
  .graph { position: relative; flex: 1; min-height: 0; overflow: hidden; background: var(--color-surface-reading); }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; cursor: grab; touch-action: none; }
  .hint { position: absolute; left: var(--space-3); bottom: var(--space-2); margin: 0; font-size: var(--text-sm); color: var(--color-text-muted); pointer-events: none; }
  .loading { margin: auto; color: var(--color-text-muted); }
</style>
