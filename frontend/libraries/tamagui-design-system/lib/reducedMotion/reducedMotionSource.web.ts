const QUERY = '(prefers-reduced-motion: reduce)';

// react-native-web's AccessibilityInfo captures this query once at import, so the query is read here when a component mounts.
export function subscribeReducedMotion(onChange: (reduced: boolean) => void): () => void {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return () => undefined;
  const query = window.matchMedia(QUERY);
  onChange(query.matches);
  const listener = (event: { matches: boolean }) => onChange(event.matches);
  query.addEventListener('change', listener);
  return () => query.removeEventListener('change', listener);
}
