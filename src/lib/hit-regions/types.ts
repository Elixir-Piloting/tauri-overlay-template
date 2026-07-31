/**
 * A rectangle in CSS pixels relative to the window's top-left corner — exactly
 * what `getBoundingClientRect()` returns. Coordinates are scaled to physical
 * screen pixels on the Rust side using the window's device-pixel ratio.
 */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
  /**
   * Metadata: whether this region may take keyboard focus. Carried to Rust with
   * the rect but never acted on by the polling loop — focus is granted by a
   * deliberate click, never by hover.
   */
  focusable: boolean;
}

/** A single rect paired with its unique id, as sent to Rust. */
export interface NamedRect {
  id: string;
  rect: Rect;
}
