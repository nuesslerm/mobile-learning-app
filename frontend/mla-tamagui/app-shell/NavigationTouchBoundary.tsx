import { MenuTouchCaptureContext } from '@lib/tamagui-design-system';
import { usePathname } from 'expo-router';
import { useRef, type ReactNode } from 'react';
import { Platform, type GestureResponderEvent } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';

type Touch = { path: string; x: number; y: number; until: number };

export function NavigationTouchBoundary({ children }: { children: ReactNode }) {
  const path = usePathname();
  const last = useRef<Touch | null>(null);

  function captureRepeat({ nativeEvent }: GestureResponderEvent) {
    const { pageX: x, pageY: y } = nativeEvent;
    const previous = last.current;
    const now = performance.now();
    if (previous && previous.path !== path && now < previous.until && Math.hypot(x - previous.x, y - previous.y) <= 30) {
      previous.until = now + 500;
      return true;
    }
    // A native push can swallow intermediate touches before delivering the rest of the same burst on the next screen.
    const inLibrary = path === '/' || path === '/blind75' || path.startsWith('/leetcode/');
    last.current = inLibrary ? { path, x, y, until: now + 1000 } : null;
    return false;
  }

  const captureTouch = Platform.OS === 'ios' ? captureRepeat : undefined;
  return <MenuTouchCaptureContext.Provider value={captureTouch}>
    <GestureHandlerRootView style={{ flex: 1 }} onStartShouldSetResponderCapture={captureTouch}>
      {children}
    </GestureHandlerRootView>
  </MenuTouchCaptureContext.Provider>;
}
