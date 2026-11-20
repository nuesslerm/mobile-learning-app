/** Native: gorhom lifts a sheet above the keyboard itself, from the keyboard events, for the inputs it knows (`Sheet.TextInput`). */
export function useSheetKeyboardLift(): number {
  return 0;
}

// no native caller: the web file's twin, so the web input typechecks against one signature
export function useReportSheetInputFocus(): (focused: boolean) => void {
  return noop;
}

function noop(): void {}
