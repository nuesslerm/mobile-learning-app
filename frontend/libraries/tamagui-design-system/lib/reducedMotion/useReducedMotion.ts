import { useEffect, useState } from 'react';

import { subscribeReducedMotion } from './reducedMotionSource';

/** Whether the person asked the system for less motion; false until the source has answered. */
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(false);
  useEffect(() => subscribeReducedMotion(setReduced), []);
  return reduced;
}
