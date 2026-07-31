"use client";

import { useState } from "react";
import { motion } from "motion/react";
import { HitRegion } from "@/lib/hit-regions";

/**
 * One HTML surface hosting two overlay elements, proving the hit-region system
 * end-to-end:
 *
 * 1. A top-anchored "island" that expands/collapses on click (Framer Motion).
 *    Its hit region follows the size animation because useHitRegion samples
 *    getBoundingClientRect() every frame.
 * 2. A draggable panel (Framer Motion `drag`). Its hit region follows the drag
 *    the same way, and it is `focusable` so the search input can take the
 *    keyboard (granted on click, never on hover).
 */
export default function OverlayPage() {
  const [expanded, setExpanded] = useState(false);
  const [panelOpen, setPanelOpen] = useState(true);

  return (
    <main className="fixed inset-0 overflow-hidden">
      <HitRegion id="island">
        <motion.button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          initial={false}
          animate={{ width: expanded ? 320 : 148, height: expanded ? 88 : 44 }}
          transition={{ type: "spring", stiffness: 320, damping: 28 }}
          className="absolute left-1/2 top-6 -translate-x-1/2 flex items-center justify-center gap-2.5 overflow-hidden rounded-full bg-black/85 px-5 text-white shadow-2xl backdrop-blur-md cursor-pointer"
        >
          <span
            className={`h-2.5 w-2.5 shrink-0 rounded-full ${
              expanded ? "bg-emerald-400" : "bg-sky-400"
            }`}
          />
          <span className="whitespace-nowrap text-sm font-medium">
            {expanded ? "Expanded island" : "Island"}
          </span>
          {expanded && (
            <span className="whitespace-nowrap text-xs text-white/60">
              click to collapse
            </span>
          )}
        </motion.button>
      </HitRegion>

      {panelOpen && (
        <HitRegion id="panel" focusable>
          <motion.div
            drag
            dragMomentum={false}
            dragElastic={0.15}
            className="absolute right-8 top-1/2 w-64 rounded-2xl border border-neutral-200 bg-white/90 p-4 text-neutral-900 shadow-2xl backdrop-blur-md cursor-grab active:cursor-grabbing"
          >
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold">Draggable panel</h2>
              <button
                type="button"
                onPointerDown={(e) => e.stopPropagation()}
                onClick={() => setPanelOpen(false)}
                className="text-xs text-neutral-400 hover:text-neutral-700 cursor-pointer"
              >
                &times;
              </button>
            </div>
            <p className="mt-2 text-xs leading-relaxed text-neutral-500">
              Drag me anywhere — the hit region follows via rAF sampling. Click
              the island to see the other region resize live.
            </p>
            <input
              placeholder="Search (click to focus)"
              className="mt-3 w-full rounded-lg border border-neutral-300 bg-white px-2 py-1.5 text-xs outline-none focus:border-sky-400"
            />
          </motion.div>
        </HitRegion>
      )}
    </main>
  );
}
