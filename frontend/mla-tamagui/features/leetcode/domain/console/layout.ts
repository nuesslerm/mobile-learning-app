export const CONSOLE_SNAP_PERCENT = 70;
export const FLOATING_CONSOLE_SNAP_PERCENT = 73.4;
export const EDITOR_ZONE_MIN_HEIGHT = 280;
export const EDITOR_ZONE_MIN_HEIGHT_UNDER_CONSOLE = 96;
export const CLOSED_BOTTOM_INSET = 24;

/** How much of the page the open sheet covers, so the editor strip above it can be scrolled whole into view. */
export function consoleInset(rootHeight: number, floating = false): number {
  const snap = floating ? FLOATING_CONSOLE_SNAP_PERCENT : CONSOLE_SNAP_PERCENT;
  return Math.round(rootHeight * (snap / 100));
}

export function editorZoneMinHeight(consoleOpen: boolean): number {
  return consoleOpen ? EDITOR_ZONE_MIN_HEIGHT_UNDER_CONSOLE : EDITOR_ZONE_MIN_HEIGHT;
}
