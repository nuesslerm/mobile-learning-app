const CHIP_MARGIN = 12;

/** Where the chip row scrolls to so the selected chip sits inside the viewport: unchanged when it already does. */
export function chipScrollTarget(chip: { x: number; width: number }, viewportWidth: number, scrollX: number): number {
  if (viewportWidth <= 0) return scrollX;
  if (chip.x - CHIP_MARGIN < scrollX) return Math.max(0, chip.x - CHIP_MARGIN);
  const right = chip.x + chip.width + CHIP_MARGIN;
  if (right > scrollX + viewportWidth) return Math.max(0, right - viewportWidth);
  return scrollX;
}
