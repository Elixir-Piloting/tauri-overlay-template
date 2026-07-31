"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  type ReactNode,
} from "react";
import type { Rect } from "./types";
import { safeInvoke } from "./invoke";

interface HitRegionContextValue {
  /** Register or update a region's bounds in the shared registry. */
  register: (id: string, rect: Rect) => void;
  /** Remove a region. Must run on unmount or a stale rect becomes a dead zone. */
  deregister: (id: string) => void;
  /** Track the DOM node of a focusable region for click-outside detection. */
  registerFocusNode: (id: string, node: HTMLElement) => void;
  deregisterFocusNode: (id: string, node: HTMLElement) => void;
}

const HitRegionContext = createContext<HitRegionContextValue | null>(null);

export function useHitRegionContext(): HitRegionContextValue {
  const ctx = useContext(HitRegionContext);
  if (!ctx) {
    throw new Error("useHitRegionContext must be used within <HitRegionProvider>");
  }
  return ctx;
}

/** Below this change (px) a rect update isn't worth re-sending to Rust. */
const EPSILON = 0.5;

/**
 * Wraps the app (put it in the root layout) and owns the shared hit-region
 * registry. The registry is flushed to Rust as a whole, once per animation
 * frame at most, so N components updating together produce ONE IPC call.
 */
export function HitRegionProvider({ children }: { children: ReactNode }) {
  const registryRef = useRef<Record<string, Rect>>({});
  const focusNodesRef = useRef<Set<HTMLElement>>(new Set());
  const flushScheduledRef = useRef(false);

  const flush = useCallback(() => {
    flushScheduledRef.current = false;
    const regions = Object.entries(registryRef.current).map(([id, rect]) => ({
      id,
      rect,
    }));
    safeInvoke("update_hit_regions", { regions });
  }, []);

  const scheduleFlush = useCallback(() => {
    if (flushScheduledRef.current) return;
    flushScheduledRef.current = true;
    requestAnimationFrame(flush);
  }, [flush]);

  const register = useCallback(
    (id: string, rect: Rect) => {
      const prev = registryRef.current[id];
      if (
        prev &&
        Math.abs(prev.x - rect.x) < EPSILON &&
        Math.abs(prev.y - rect.y) < EPSILON &&
        Math.abs(prev.width - rect.width) < EPSILON &&
        Math.abs(prev.height - rect.height) < EPSILON &&
        prev.focusable === rect.focusable
      ) {
        return;
      }
      registryRef.current[id] = rect;
      scheduleFlush();
    },
    [scheduleFlush]
  );

  const deregister = useCallback(
    (id: string) => {
      if (!(id in registryRef.current)) return;
      delete registryRef.current[id];
      scheduleFlush();
    },
    [scheduleFlush]
  );

  const registerFocusNode = useCallback((_id: string, node: HTMLElement) => {
    focusNodesRef.current.add(node);
  }, []);

  const deregisterFocusNode = useCallback((_id: string, node: HTMLElement) => {
    focusNodesRef.current.delete(node);
  }, []);

  // Click-outside of any focusable region -> release overlay focus. Only
  // reachable while the cursor is over some region (pass-through is off there),
  // e.g. the user clicks a non-focusable part of the overlay.
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      const insideFocusable = [...focusNodesRef.current].some((node) =>
        node.contains(target)
      );
      if (!insideFocusable) {
        safeInvoke("set_overlay_focus", { focused: false });
      }
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, []);

  const value = useMemo(
    () => ({
      register,
      deregister,
      registerFocusNode,
      deregisterFocusNode,
    }),
    [register, deregister, registerFocusNode, deregisterFocusNode]
  );

  return <HitRegionContext.Provider value={value}>{children}</HitRegionContext.Provider>;
}
