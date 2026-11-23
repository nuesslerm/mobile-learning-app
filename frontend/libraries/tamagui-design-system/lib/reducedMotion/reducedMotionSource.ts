import { AccessibilityInfo } from 'react-native';

/** Native source: the system's reduce-motion setting, read once and then followed. Returns the unsubscribe. */
export function subscribeReducedMotion(onChange: (reduced: boolean) => void): () => void {
  let live = true;
  void AccessibilityInfo.isReduceMotionEnabled().then((value) => {
    if (live) onChange(value);
  });
  const subscription = AccessibilityInfo.addEventListener('reduceMotionChanged', onChange);
  return () => {
    live = false;
    subscription.remove();
  };
}
