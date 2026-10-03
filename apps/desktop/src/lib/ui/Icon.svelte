<script lang="ts">
  import type { Component } from "svelte";

  type Size = "sm" | "md" | "lg";
  const PX: Record<Size, number> = { sm: 14, md: 16, lg: 20 };
  let { icon: Glyph, size = "md", label }: { icon: Component<any>; size?: Size; label?: string } = $props();
</script>

<!-- Decorative unless a label is given (then it is an image with that name). -->
<Glyph size={PX[size]} strokeWidth={1.75} aria-hidden={label ? undefined : "true"} role={label ? "img" : undefined} aria-label={label} />
