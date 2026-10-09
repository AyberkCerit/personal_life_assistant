<script lang="ts">
  // A panel border the user drags to resize the panel (like Obsidian). It lights up in the accent
  // colour on hover and while dragging; arrow keys move it too, double-click restores the default.
  interface Props {
    side: "left" | "right";
    width: number;
    min: number;
    max: number;
    label: string;
    onResize: (width: number) => void;
    onDone: () => void;
    onReset: () => void;
  }

  let { side, width, min, max, label, onResize, onDone, onReset }: Props = $props();
  let dragging = $state(false);
  let startX = 0;
  let startWidth = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault(); // no text selection while dragging
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true;
    startX = e.clientX;
    startWidth = width;
  }

  function move(e: PointerEvent) {
    if (!dragging) return;
    const dx = e.clientX - startX;
    onResize(startWidth + (side === "left" ? dx : -dx));
  }

  function up() {
    if (!dragging) return;
    dragging = false;
    onDone();
  }

  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16;
    const grow = side === "left" ? "ArrowRight" : "ArrowLeft";
    const shrink = side === "left" ? "ArrowLeft" : "ArrowRight";
    if (e.key === grow) onResize(width + step);
    else if (e.key === shrink) onResize(width - step);
    else if (e.key === "Home") onResize(min);
    else if (e.key === "End") onResize(max);
    else if (e.key === "Enter") onReset();
    else return;
    e.preventDefault();
    onDone();
  }
</script>

<!-- a focusable separator is the ARIA pattern for a splitter -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="splitter {side}"
  class:dragging
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  title={label}
  aria-valuenow={width}
  aria-valuemin={min}
  aria-valuemax={max}
  tabindex="0"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  ondblclick={() => {
    onReset();
    onDone();
  }}
  onkeydown={key}
></div>

<style>
  .splitter {
    position: absolute; top: 0; bottom: 1.75rem; width: 9px; z-index: 5;
    cursor: col-resize; touch-action: none;
  }
  .splitter.left { left: calc(var(--left-w) - 5px); }
  .splitter.right { right: calc(var(--right-w) - 5px); }
  /* the visible line sits on the panel border and turns the accent colour */
  .splitter::after {
    content: ""; position: absolute; top: 0; bottom: 0; left: 3px; width: 3px; border-radius: 2px;
    background: var(--color-accent); opacity: 0;
    transition: opacity var(--duration-fast) var(--ease-standard);
  }
  .splitter:hover::after, .splitter.dragging::after { opacity: 1; }
  .splitter:focus-visible { outline: none; }
  .splitter:focus-visible::after { opacity: 1; }
</style>
