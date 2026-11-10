/**
 * Pure geometry for wireframe annotations. No React / RN / IO imports here.
 */
import type { Rect, WireframePlacement } from '../domain/types';

export interface Viewport {
  width: number;
  height: number;
}

export interface WireframeRect extends Rect {
  componentType: string;
  scrollY?: number;
}

/** Default wireframe box size in logical pixels. */
export const DEFAULT_WIREFRAME_WIDTH = 160;
export const DEFAULT_WIREFRAME_HEIGHT = 96;

/** Convert a screen rect + viewport into viewport-relative ratios. */
export function rectToWireframe(
  rect: Rect,
  viewport: Viewport,
  componentType: string,
  scrollY = 0,
): WireframePlacement {
  return {
    componentType,
    xRatio: clamp(rect.x / viewport.width),
    yRatio: clamp((rect.y + scrollY) / viewport.height),
    widthRatio: clamp(rect.width / viewport.width),
    heightRatio: clamp(rect.height / viewport.height),
    scrollY,
  };
}

/** Create a wireframe placement centered at a click point with default size. */
export function pointToWireframe(
  x: number,
  y: number,
  viewport: Viewport,
  componentType: string,
): WireframePlacement {
  const halfW = DEFAULT_WIREFRAME_WIDTH / 2;
  const halfH = DEFAULT_WIREFRAME_HEIGHT / 2;
  const left = Math.max(0, Math.min(x - halfW, viewport.width - DEFAULT_WIREFRAME_WIDTH));
  const top = Math.max(0, Math.min(y - halfH, viewport.height - DEFAULT_WIREFRAME_HEIGHT));
  return rectToWireframe(
    { x: left, y: top, width: DEFAULT_WIREFRAME_WIDTH, height: DEFAULT_WIREFRAME_HEIGHT },
    viewport,
    componentType,
  );
}

/** Round a ratio to a readable percentage (1 decimal). */
export function ratioToPercent(ratio: number): string {
  return `${(ratio * 100).toFixed(1)}%`;
}

/** Pixel distance between the bottom of prev and top of next. */
export function verticalGap(
  prev: WireframePlacement,
  next: WireframePlacement,
  viewportHeight: number,
): number {
  const prevBottomPx = (prev.yRatio + prev.heightRatio) * viewportHeight;
  const nextTopPx = next.yRatio * viewportHeight;
  return Math.round(nextTopPx - prevBottomPx);
}

function clamp(n: number): number {
  return Math.max(0, Math.min(1, n));
}
