const REDUCED_MOTION_QUERY = '(prefers-reduced-motion: reduce)';

type Listener = (event: { matches: boolean; media: string }) => void;

const listeners = new Set<Listener>();
let reduced = false;

// One persistent object: the design system reads the query when a component mounts and then listens on it, so a test flips the preference through it.
const reducedMotionQuery = {
  get matches() {
    return reduced;
  },
  media: REDUCED_MOTION_QUERY,
  onchange: null,
  addListener: (listener: Listener) => void listeners.add(listener),
  removeListener: (listener: Listener) => void listeners.delete(listener),
  addEventListener: (_type: string, listener: Listener) => void listeners.add(listener),
  removeEventListener: (_type: string, listener: Listener) => void listeners.delete(listener),
  dispatchEvent: () => false,
};

/** The `window.matchMedia` the flow tests run on: the reduced-motion query is controllable, every other query answers false. */
export function flowMatchMedia(query: string): MediaQueryList {
  if (query === REDUCED_MOTION_QUERY) return reducedMotionQuery as unknown as MediaQueryList;
  return {
    matches: false,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => false,
  };
}

/** Sets the person's reduce-motion preference; a component that is already listening follows. */
export function setReducedMotion(value: boolean): void {
  reduced = value;
  for (const listener of [...listeners]) listener({ matches: value, media: REDUCED_MOTION_QUERY });
}

export function resetReducedMotion(): void {
  reduced = false;
  listeners.clear();
}
