import { MenuTouchCaptureContext } from '@lib/tamagui-design-system';
import { usePathname } from 'expo-router';
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { Platform, View, type GestureResponderEvent } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';
import { FullWindowOverlay } from 'react-native-screens';

type Touch = { path: string; x: number; y: number; until: number };

export function NavigationTouchBoundary({ children }: { children: ReactNode }) {
  const path = usePathname();
  const last = useRef<Touch | null>(null);

  const [sheetTouch, setSheetTouch] = useState<Touch | null>(null);

  useLayoutEffect(() => {
    const origin = last.current;
    if (Platform.OS !== 'ios' || !/^\/leetcode\/[^/]+\/(history|description)$/.test(path)
        || !origin || origin.path === path || origin.until <= performance.now()) {
      setSheetTouch(null);
      return;
    }

    // The sheet backdrop has a native tap recognizer that runs before React responder capture.
    setSheetTouch(origin);
    let timeout: ReturnType<typeof setTimeout>;
    function expire() {
      const remaining = (origin?.until ?? 0) - performance.now();
      if (last.current === origin && remaining > 0) timeout = setTimeout(expire, remaining);
      else setSheetTouch(null);
    }
    expire();
    return () => clearTimeout(timeout);
  }, [path]);

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
      {sheetTouch ? (
        <FullWindowOverlay unstable_accessibilityContainerViewIsModal={false}>
          <View
            accessible={false}
            collapsable={false}
            onStartShouldSetResponder={() => true}
            onResponderGrant={() => { sheetTouch.until = performance.now() + 500; }}
            style={{ position: 'absolute', left: sheetTouch.x - 30, top: sheetTouch.y - 30, width: 60, height: 60 }}
          />
        </FullWindowOverlay>
      ) : null}
    </GestureHandlerRootView>
  </MenuTouchCaptureContext.Provider>;
}
