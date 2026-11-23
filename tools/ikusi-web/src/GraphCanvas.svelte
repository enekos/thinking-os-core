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
  import type { Meta, Scene } from './api';
  import { nodeHref } from './navigation';
  import { Plot, colors, hitTest, paint, type Dot } from './plot';
  let {
    scene,
    meta,
    focus,
    hidden,
    theme,
  }: { scene: Scene; meta: Meta; focus: string; hidden: Set<string>; theme: string } = $props();
  let width = $state(0),
    height = $state(0);
  let canvas: HTMLCanvasElement = $state()!;
  const plot = $derived(new Plot(scene, meta, hidden, focus));
  const palette = $derived.by(() => {
    void theme;
    return canvas && colors(canvas);
  });
  let view = $state.raw({ x: 0, y: 0, k: 1 });
  let dots: ReturnType<typeof paint> = [];
  let hover = $state.raw<Dot | undefined>();
  let drag: { id: number; x: number; y: number; ox: number; oy: number; moved: boolean } | undefined;
  export function recenter() {
    view = plot.fit(width, height);
    hover = undefined;
  }
  $effect(() => {
    void plot;
    void width;
    void height;
    untrack(recenter);
  });
  $effect(() => {
    const target = canvas,
      graph = plot,
      ink = palette,
      camera = view,
      w = width,
      h = height;
    if (!target || !ink || !w || !h) return;
    const frame = requestAnimationFrame(() => {
      dots = paint(target, graph, ink, camera, w, h);
    });
    return () => cancelAnimationFrame(frame);
  });
  function zoom(factor: number, x = width / 2, y = height / 2) {
    const k = Math.max(0.001, Math.min(20, view.k * factor));
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
      hover = hit;
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
    if (!hit) return;
    if (hit.mark.count === 1) location.hash = nodeHref(hit.mark.node.id);
    else {
      const k = Math.min(20, view.k * 2.8);
      view = { k, x: width / 2 - hit.mark.x * k, y: height / 2 - hit.mark.y * k };
      hover = undefined;
    }
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
      {#if hover.mark.count > 1}<strong>{hover.mark.count} nearby nodes</strong><small>
          Select to zoom in
        </small>
      {:else}<strong>{hover.mark.node.n}</strong><small>
          {hover.mark.node.t} · {hover.mark.node.d} connections
        </small>{#if hover.mark.node.s}<span>{hover.mark.node.s}</span>{/if}{/if}
    </div>{/if}
  <div class="plot-zoom">
    <Button size="sm" label="Zoom out" icon="minus" flat onclick={() => zoom(1 / 1.4)} />
    <Button size="sm" label="Zoom in" icon="plus" flat onclick={() => zoom(1.4)} />
    <Button size="sm" label="Fit graph" icon="maximize" flat onclick={recenter} />
  </div>
</div>
