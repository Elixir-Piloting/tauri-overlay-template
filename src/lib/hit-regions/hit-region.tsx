"use client";

import {
  Children,
  cloneElement,
  type DOMAttributes,
  type PointerEvent,
  type ReactElement,
  type ReactNode,
  type Ref,
} from "react";
import { useHitRegion } from "./use-hit-region";
import { setOverlayFocus } from "./invoke";

export interface HitRegionProps {
  /** Unique id for this region. Must be unique across all mounted regions. */
  id: string;
  /** Whether the region may take keyboard focus. Defaults to false. */
  focusable?: boolean;
  /** Optional user handler, merged with the focus-grant handler. */
  onPointerDown?: (event: PointerEvent<HTMLElement>) => void;
  /** Exactly one element — the interactive root of the region. */
  children: ReactNode;
}

/**
 * The primary developer-facing hit-region API. Wrap the interactive root element
 * of an overlay UI and it becomes click-capturing while the cursor is over it
 * (and click-through everywhere else).
 *
 * Requires exactly one child element; the region ref is forwarded to it so
 * bounds are measured on the element itself — which keeps Framer Motion
 * drag/animate transforms correct.
 *
 * Pass `focusable` for regions that need keyboard input (search boxes, text
 * inputs). A real click inside the region grants the window focus; hovering
 * never does, so the cursor drifting over a region can't steal the user's
 * keyboard input from another app.
 */
export function HitRegion({ id, focusable = false, onPointerDown, children }: HitRegionProps) {
  const { ref } = useHitRegion(id, { focusable });

  const child = Children.only(children) as ReactElement;

  const mergedProps: Partial<DOMAttributes<HTMLElement>> = focusable
    ? {
        onPointerDown: (event: PointerEvent<HTMLElement>) => {
          setOverlayFocus(true);
          onPointerDown?.(event);
        },
      }
    : {};

  // React 19 treats `ref` as a regular prop; type the props object explicitly so
  // cloneElement accepts it (its `Attributes` type only models `key`).
  const props: Partial<DOMAttributes<HTMLElement>> & { ref?: Ref<HTMLElement> } = {
    ...mergedProps,
    ref,
  };

  return cloneElement(child, props);
}
