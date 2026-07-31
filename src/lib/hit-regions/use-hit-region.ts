"use client";

import { useEffect, useRef } from "react";
import { useHitRegionContext } from "./hit-region-context";
import { setOverlayFocus } from "./invoke";

interface UseHitRegionOptions {
  /** Whether this region may take keyboard focus (granted on click, not hover). */
  focusable?: boolean;
}

/**
 * Attach the returned `ref` to any DOM element to make it a hit region.
 *
 * - Measures via `ResizeObserver` for size/layout changes.
 * - Also samples `getBoundingClientRect()` every animation frame so the region
 *   follows transform-driven motion (Framer Motion `animate`/`drag`).
 * - Deregisters its own id on unmount so no stale rect is left behind — a stale
 *   rect would become an invisible permanent dead zone.
 * - Registers its DOM node so the provider can release focus on click-outside,
 *   and releases focus itself when a focusable region unmounts.
 */
export function useHitRegion(id: string, options?: UseHitRegionOptions) {
  const { register, deregister, registerFocusNode, deregisterFocusNode } =
    useHitRegionContext();

  const focusable = options?.focusable ?? false;

  const ref = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;

    const update = () => {
      const el = ref.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      register(id, {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        focusable,
      });
    };

    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);

    let raf = 0;
    const sample = () => {
      update();
      raf = requestAnimationFrame(sample);
    };
    raf = requestAnimationFrame(sample);

    if (focusable) {
      registerFocusNode(id, element);
    }

    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
      if (focusable) {
        deregisterFocusNode(id, element);
        setOverlayFocus(false);
      }
      deregister(id);
    };
  }, [id, focusable, register, deregister, registerFocusNode, deregisterFocusNode]);

  return { ref };
}
