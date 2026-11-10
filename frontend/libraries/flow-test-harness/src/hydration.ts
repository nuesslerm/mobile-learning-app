/** React reports a hydration mismatch as an error event on the window, which fails the run unless a listener has it. */
export function hydrationErrors(): { seen: string[]; stop: () => void } {
  const seen: string[] = [];
  const onError = (event: ErrorEvent) => {
    seen.push(String(event.error?.message ?? event.message));
    event.preventDefault();
  };
  window.addEventListener('error', onError);
  return { seen, stop: () => window.removeEventListener('error', onError) };
}
