<script module lang="ts">
  // The wheel listener must be non-passive so the page stays still while zooming the map.
  function bindWheel(el: HTMLCanvasElement, fn: (event: WheelEvent) => void) {
    el.addEventListener('wheel', fn, { passive: false });
    return { destroy: () => el.removeEventListener('wheel', fn) };
  }
</script>

<script lang="ts">
  import { Button } from '@enekos/osagai';
  import { untrack } from 'svelte';
  import type { Meta, Point, Scene } from './api';
  import { nodeHref } from './navigation';
  import { fit, hitTest, paint } from './plot';
  let {
    scene,
    meta,
    focus,
    hidden,
    theme,
  }: { scene: Scene; meta: Meta; focus: string; hidden: Set<string>; theme: string } = $props();
  let width = $state(0),
    height = $state(0);
  let canvas: HTMLCanvasElement;
  let view = $state({ x: 0, y: 0, k: 1 });
  let dots: ReturnType<typeof paint> = [];
  let hover = $state<{ node: Point; x: number; y: number } | undefined>();
  let drag: { id: number; x: number; y: number; ox: number; oy: number; moved: boolean } | undefined;
  export function recenter() {
    const shown = scene.nodes.filter((n) => !hidden.has(n.t));
    view = fit(shown, width, height);
    hover = undefined;
  }
  $effect(() => {
    void scene;
    void width;
    void height;
    void hidden;
    untrack(recenter);
  });
  $effect(() => {
    void theme;
    dots = paint(canvas, scene, meta, focus, hidden, view, width, height);
  });
  function zoom(factor: number, x = width / 2, y = height / 2) {
    const k = Math.max(0.01, Math.min(20, view.k * factor));
    view = { k, x: x - ((x - view.x) * k) / view.k, y: y - ((y - view.y) * k) / view.k };
    hover = undefined;
  }
  function wheel(e: WheelEvent) {
    e.preventDefault();
    zoom(Math.exp(-e.deltaY * 0.0018), e.offsetX, e.offsetY);
  }
  function pointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    drag = { id: e.pointerId, x: e.clientX, y: e.clientY, ox: view.x, oy: view.y, moved: false };
    canvas.setPointerCapture(e.pointerId);
  }
  function pointerMove(e: PointerEvent) {
    if (!drag) {
      const hit = hitTest(dots, e.offsetX, e.offsetY);
      hover = hit && { node: hit.node, x: e.offsetX, y: e.offsetY };
      return;
    }
    if (e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x,
      dy = e.clientY - drag.y;
    if (Math.hypot(dx, dy) > 4) drag.moved = true;
    if (drag.moved) {
      view = { ...view, x: drag.ox + dx, y: drag.oy + dy };
      hover = undefined;
    }
  }
  function pointerUp(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const hit = !drag.moved && hitTest(dots, e.offsetX, e.offsetY);
    drag = undefined;
    if (hit) location.hash = nodeHref(hit.node.id);
  }
</script>

<div class="plot" bind:clientWidth={width} bind:clientHeight={height}>
  <canvas
    bind:this={canvas}
    aria-label="Connection map. Open a node with search or the node list."
    class:pointing={!!hover}
    onpointerdown={pointerDown}
    onpointermove={pointerMove}
    onpointerup={pointerUp}
    onpointerleave={() => (hover = undefined)}
    onpointercancel={() => (drag = undefined)}
    use:bindWheel={wheel}
  ></canvas>
  {#if hover}<div
      class="graph-tip"
      style:left={Math.max(6, Math.min(hover.x + 12, width - 228)) + 'px'}
      style:top={Math.max(6, Math.min(hover.y + 12, height - 100)) + 'px'}
    >
      <strong>{hover.node.n}</strong><small>
        {hover.node.t} · {hover.node.d} connections
      </small>{#if hover.node.s}<span>{hover.node.s}</span>{/if}
    </div>{/if}
  <div class="plot-zoom">
    <Button size="sm" label="Zoom out" icon="minus" flat onclick={() => zoom(1 / 1.4)} />
    <Button size="sm" label="Zoom in" icon="plus" flat onclick={() => zoom(1.4)} />
    <Button size="sm" label="Fit graph" icon="maximize" flat onclick={recenter} />
  </div>
</div>
