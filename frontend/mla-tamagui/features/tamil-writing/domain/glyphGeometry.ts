import type { Point } from './strokes';

/**
 * The printed letter, as geometry the code can actually measure against.
 *
 * This exists because of a real failure: the first stroke guides were drawn by
 * hand, the validator marked a learner's stroke against those same paths, and
 * every test passed -- while the reference for அ was a hexagonal C that looked
 * nothing like அ. Nothing in the system ever compared the reference to the
 * letter, so nothing could catch it.
 *
 * With the glyph's own outline in hand, "does this path follow the letter" is
 * a measurement, not an opinion.
 *
 * Coordinates are fractions of the glyph's em box, y downward, matching the
 * tracing pad.
 */

/** One closed ring of the outline, already flattened from its curves. */
export type GlyphContour = readonly Point[];

export type GlyphOutline = {
  /** Outer rings and holes together; filled by the even-odd rule. */
  contours: readonly GlyphContour[];
};

export type GlyphRaster = {
  size: number;
  /** `size * size`, row-major. 1 where the letter has ink. */
  ink: Uint8Array;
  inkCount: number;
};

/**
 * 96 after trying 160: a finer grid does not give a cleaner skeleton, it gives
 * a bushier one -- more branch points per letter, so `chainIntoStrokes` stops
 * more often and ஓ went from 5 "strokes" to 10. The letterform is the same at
 * both; only the artefacts scale.
 */
export const DEFAULT_RASTER_SIZE = 96;

/**
 * Is this point inside the letter, by the NONZERO winding rule?
 *
 * Not even-odd, and the difference is the whole letterform: a TrueType glyph
 * is filled nonzero, and its contours are free to overlap. Filled even-odd,
 * every overlap becomes a hole -- which is exactly what shipped: அ had a white
 * notch through the middle where its bowl crosses its stem, the raster had a
 * gap there, and the skeleton dutifully split into two strokes on either side
 * of a hole that does not exist.
 */
export function isInsideOutline(outline: GlyphOutline, point: Point): boolean {
  let winding = 0;
  for (const contour of outline.contours) {
    for (let i = 0, j = contour.length - 1; i < contour.length; j = i, i += 1) {
      const a = contour[i];
      const b = contour[j];
      if (!a || !b) continue;
      if (a.y <= point.y) {
        if (b.y > point.y && cross(a, b, point) > 0) winding += 1;
      } else if (b.y <= point.y && cross(a, b, point) < 0) {
        winding -= 1;
      }
    }
  }
  return winding !== 0;
}

/** Which side of the directed edge a->b the point falls on. */
function cross(a: Point, b: Point, point: Point): number {
  return (b.x - a.x) * (point.y - a.y) - (point.x - a.x) * (b.y - a.y);
}

/** The outline filled into a bitmap, sampled at pixel centres. */
export function rasterizeGlyph(
  outline: GlyphOutline,
  size: number = DEFAULT_RASTER_SIZE
): GlyphRaster {
  const ink = new Uint8Array(size * size);
  let inkCount = 0;
  for (let row = 0; row < size; row += 1) {
    for (let column = 0; column < size; column += 1) {
      const point = { x: (column + 0.5) / size, y: (row + 0.5) / size };
      if (isInsideOutline(outline, point)) {
        ink[row * size + column] = 1;
        inkCount += 1;
      }
    }
  }
  return { size, ink, inkCount };
}

function isInk(raster: GlyphRaster, point: Point): boolean {
  const column = Math.floor(point.x * raster.size);
  const row = Math.floor(point.y * raster.size);
  if (column < 0 || row < 0 || column >= raster.size || row >= raster.size) {
    return false;
  }
  return raster.ink[row * raster.size + column] === 1;
}

/** Pixel centre of a raster cell, in 0..1 coordinates. */
function pixelCentre(
  raster: GlyphRaster,
  column: number,
  row: number
): Point {
  return { x: (column + 0.5) / raster.size, y: (row + 0.5) / raster.size };
}

export function outlineBounds(outline: GlyphOutline): {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
} {
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  for (const contour of outline.contours) {
    for (const point of contour) {
      minX = Math.min(minX, point.x);
      minY = Math.min(minY, point.y);
      maxX = Math.max(maxX, point.x);
      maxY = Math.max(maxY, point.y);
    }
  }
  return { minX, minY, maxX, maxY };
}

/**
 * How much of a stroke path lies on the letter: 1 means every sample of it is
 * inside the ink.
 *
 * This is the check the hand-drawn guides could never have passed, and the
 * reason they survived anyway: the validator compared a learner's stroke to
 * the reference, and nothing compared the reference to the letter.
 */
export function pathOnGlyph(
  raster: GlyphRaster,
  path: readonly Point[],
  samplesPerSegment = 6
): number {
  let inside = 0;
  let total = 0;
  for (let i = 1; i < path.length; i += 1) {
    const from = path[i - 1];
    const to = path[i];
    if (!from || !to) continue;
    for (let step = 0; step < samplesPerSegment; step += 1) {
      const t = step / samplesPerSegment;
      total += 1;
      if (
        isNearInk(raster, {
          x: from.x + (to.x - from.x) * t,
          y: from.y + (to.y - from.y) * t,
        })
      ) {
        inside += 1;
      }
    }
  }
  return total === 0 ? 0 : inside / total;
}

/**
 * Ink at this point or in the cell next to it.
 *
 * A pixel grid has corners: two diagonally adjacent ink pixels are one line to
 * a human and the segment between their centres passes exactly through an
 * empty corner cell. Measured strictly, a stroke made of skeleton pixels --
 * every one of them ink -- scores 94%. The one-cell reach is the raster's own
 * quantisation, not slack in the measurement: a stroke off the letter is still
 * off it by far more than a cell.
 */
function isNearInk(raster: GlyphRaster, point: Point): boolean {
  if (isInk(raster, point)) return true;
  const column = Math.floor(point.x * raster.size);
  const row = Math.floor(point.y * raster.size);
  for (let dy = -1; dy <= 1; dy += 1) {
    for (let dx = -1; dx <= 1; dx += 1) {
      const nextColumn = column + dx;
      const nextRow = row + dy;
      if (
        nextColumn < 0 ||
        nextRow < 0 ||
        nextColumn >= raster.size ||
        nextRow >= raster.size
      ) {
        continue;
      }
      if (raster.ink[nextRow * raster.size + nextColumn] === 1) return true;
    }
  }
  return false;
}

/**
 * How much of the letter the strokes account for: 1 means every inked pixel is
 * within `penRadius` of some stroke.
 *
 * The complement of `pathOnGlyph`, and both are needed. A one-pixel dot in the
 * middle of the letter scores a perfect 1 on the first and nearly 0 here.
 */
export function inkCoverage(
  raster: GlyphRaster,
  paths: readonly (readonly Point[])[],
  penRadius: number
): number {
  if (raster.inkCount === 0) return 0;
  let covered = 0;
  for (let row = 0; row < raster.size; row += 1) {
    for (let column = 0; column < raster.size; column += 1) {
      if (raster.ink[row * raster.size + column] !== 1) continue;
      const point = pixelCentre(raster, column, row);
      if (paths.some((path) => nearPath(point, path, penRadius))) covered += 1;
    }
  }
  return covered / raster.inkCount;
}

function nearPath(
  point: Point,
  path: readonly Point[],
  radius: number
): boolean {
  for (let i = 1; i < path.length; i += 1) {
    const a = path[i - 1];
    const b = path[i];
    if (!a || !b) continue;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const lengthSquared = dx * dx + dy * dy;
    const t =
      lengthSquared === 0
        ? 0
        : Math.max(
            0,
            Math.min(
              1,
              ((point.x - a.x) * dx + (point.y - a.y) * dy) / lengthSquared
            )
          );
    const nearest = { x: a.x + dx * t, y: a.y + dy * t };
    if (Math.hypot(point.x - nearest.x, point.y - nearest.y) <= radius) {
      return true;
    }
  }
  return false;
}

export type InkCluster = {
  /** Centre of the uncovered patch. */
  centre: Point;
  /** Half its longer side. */
  radius: number;
  pixels: number;
};

/**
 * Patches of the letter that no stroke accounts for, grouped.
 *
 * Thinning a round dot leaves a single pixel, which is not a path, so a letter
 * like ஈ loses its two bowls entirely and the strokes cover 83% of it. These
 * are what has to be added back as a stroke of its own.
 */
export function uncoveredClusters(
  raster: GlyphRaster,
  paths: readonly (readonly Point[])[],
  penRadius: number,
  minPixels = 4
): InkCluster[] {
  const { size } = raster;
  const open = new Uint8Array(size * size);
  for (let row = 0; row < size; row += 1) {
    for (let column = 0; column < size; column += 1) {
      const index = row * size + column;
      if (raster.ink[index] !== 1) continue;
      const point = pixelCentre(raster, column, row);
      if (!paths.some((path) => nearPath(point, path, penRadius))) {
        open[index] = 1;
      }
    }
  }

  const clusters: InkCluster[] = [];
  const seen = new Uint8Array(size * size);

  for (let row = 0; row < size; row += 1) {
    for (let column = 0; column < size; column += 1) {
      const start = row * size + column;
      if (open[start] !== 1 || seen[start] === 1) continue;

      const queue = [{ column, row }];
      seen[start] = 1;
      const members: Point[] = [];

      while (queue.length > 0) {
        const cell = queue.pop();
        if (!cell) break;
        members.push(pixelCentre(raster, cell.column, cell.row));
        for (let dy = -1; dy <= 1; dy += 1) {
          for (let dx = -1; dx <= 1; dx += 1) {
            const nextColumn: number = cell.column + dx;
            const nextRow: number = cell.row + dy;
            if (
              nextColumn < 0 ||
              nextRow < 0 ||
              nextColumn >= size ||
              nextRow >= size
            ) {
              continue;
            }
            const index = nextRow * size + nextColumn;
            if (open[index] === 1 && seen[index] === 0) {
              seen[index] = 1;
              queue.push({ column: nextColumn, row: nextRow });
            }
          }
        }
      }

      if (members.length < minPixels) continue;
      const minX = Math.min(...members.map((p) => p.x));
      const maxX = Math.max(...members.map((p) => p.x));
      const minY = Math.min(...members.map((p) => p.y));
      const maxY = Math.max(...members.map((p) => p.y));
      clusters.push({
        centre: { x: (minX + maxX) / 2, y: (minY + maxY) / 2 },
        radius: Math.max(maxX - minX, maxY - minY) / 2,
        pixels: members.length,
      });
    }
  }

  return clusters.sort((a, b) => b.pixels - a.pixels);
}
