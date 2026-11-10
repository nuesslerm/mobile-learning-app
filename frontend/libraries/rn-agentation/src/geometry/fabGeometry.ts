// fabGeometry.ts
export interface Insets {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

export interface FabLayout {
  width: number;
  height: number;
  insets: Insets;
  fabSize: number;
  margin: number;
}

export interface FabBounds {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

export function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(Math.max(value, 0), 1);
}

export function fabBounds({ width, height, insets, fabSize, margin }: FabLayout): FabBounds {
  return {
    minX: insets.left + margin,
    maxX: width - fabSize - insets.right - margin,
    minY: insets.top + margin,
    maxY: height - fabSize - insets.bottom - margin,
  };
}

function lerp(min: number, max: number, ratio: number): number {
  return min + (max - min) * ratio;
}

function inverseLerp(min: number, max: number, value: number): number {
  if (max <= min) return 0;
  return clamp01((value - min) / (max - min));
}

export function ratioToPosition(
  xRatio: number,
  yRatio: number,
  b: FabBounds,
): { x: number; y: number } {
  return { x: lerp(b.minX, b.maxX, clamp01(xRatio)), y: lerp(b.minY, b.maxY, clamp01(yRatio)) };
}

export function positionToRatio(
  x: number,
  y: number,
  b: FabBounds,
): { xRatio: number; yRatio: number } {
  return { xRatio: inverseLerp(b.minX, b.maxX, x), yRatio: inverseLerp(b.minY, b.maxY, y) };
}
