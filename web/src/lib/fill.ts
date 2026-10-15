// Svelte action that sizes an element to fill the rest of the viewport
// below it (log views, consoles and editors), with a minimum height.

export interface FillOptions {
  /** Space to leave below the element, in pixels. */
  bottom?: number;
  /** Never shrink below this height, in pixels. */
  min?: number;
}

export function fillHeight(node: HTMLElement, opts: FillOptions = {}) {
  let { bottom = 18, min = 320 } = opts;
  let frame = 0;

  const update = () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const top = node.getBoundingClientRect().top;
      node.style.height = `${Math.max(min, Math.floor(window.innerHeight - Math.max(top, 0) - bottom))}px`;
    });
  };

  update();
  window.addEventListener('resize', update);
  // Layout above the element can change (banners, wrapping toolbars).
  const ro = new ResizeObserver(update);
  if (node.parentElement) ro.observe(node.parentElement);

  return {
    update(next: FillOptions = {}) {
      ({ bottom = 18, min = 320 } = next);
      update();
    },
    destroy() {
      cancelAnimationFrame(frame);
      window.removeEventListener('resize', update);
      ro.disconnect();
    },
  };
}
