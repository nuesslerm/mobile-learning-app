import { useEffect, useRef, useState } from 'react';
import { Image, View, type GestureResponderEvent } from 'react-native';

const initial = { zoom: 1, left: 0, top: 0 };
type Props = {
  uri: string;
  onInteraction: () => void;
  viewport: { width: number; height: number };
  onLayout: (size: { width: number; height: number }) => void;
  click: (x: number, y: number, right: boolean) => void;
  wheel: (dx: number, dy: number) => void;
};

export function BrowserViewport({
  uri,
  onInteraction,
  viewport,
  onLayout,
  click,
  wheel,
}: Props) {
  const [transform, setTransform] = useState(initial);
  const position = useRef(initial);
  const gesture = useRef({ x: 0, y: 0, lastX: 0, lastY: 0, moved: false });
  const pinch = useRef<{
    distance: number;
    zoom: number;
    x: number;
    y: number;
  } | null>(null);
  const hold = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const clearHold = () => clearTimeout(hold.current);
  useEffect(() => clearHold, []);
  function move(zoom: number, x: number, y: number) {
    const left = Math.max(viewport.width * (1 - zoom), Math.min(0, x));
    const top = Math.max(viewport.height * (1 - zoom), Math.min(0, y));
    position.current = { zoom, left, top };
    setTransform(position.current);
  }
  function tap(x: number, y: number, right: boolean) {
    const { left, top, zoom } = position.current;
    click((x - left) / zoom, (y - top) / zoom, right);
  }
  function track(event: GestureResponderEvent) {
    const { touches, locationX: x, locationY: y } = event.nativeEvent;
    const { zoom, left, top } = position.current;
    const t = gesture.current;
    if (touches.length >= 2) {
      clearHold();
      t.moved = true;
      const [a, b] = touches;
      const distance = Math.max(
        1,
        Math.hypot(a.pageX - b.pageX, a.pageY - b.pageY)
      );
      const midX = (a.locationX + b.locationX) / 2;
      const midY = (a.locationY + b.locationY) / 2;
      const start = (pinch.current ??= {
        distance,
        zoom,
        x: (midX - left) / zoom,
        y: (midY - top) / zoom,
      });
      const next = Math.max(
        1,
        Math.min(3.5, (start.zoom * distance) / start.distance)
      );
      move(next, midX - start.x * next, midY - start.y * next);
      return;
    }
    if (pinch.current || (!t.moved && Math.hypot(x - t.x, y - t.y) <= 5))
      return;
    clearHold();
    t.moved = true;
    if (zoom > 1) move(zoom, left + x - t.lastX, top + y - t.lastY);
    else wheel(t.lastX - x, t.lastY - y);
    t.lastX = x;
    t.lastY = y;
  }
  return (
    <View
      accessibilityLabel="Desktop browser viewport"
      style={{ flex: 1, overflow: 'hidden' }}
      onLayout={(e) => {
        const { width, height } = e.nativeEvent.layout;
        if (
          width > 0 &&
          height > 0 &&
          (width !== viewport.width || height !== viewport.height)
        ) {
          move(1, 0, 0);
          onLayout({ width, height });
        }
      }}
      onStartShouldSetResponder={() => true}
      onResponderGrant={(e) => {
        onInteraction();
        clearHold();
        pinch.current = null;
        const { locationX: x, locationY: y } = e.nativeEvent;
        gesture.current = { x, y, lastX: x, lastY: y, moved: false };
        if (e.nativeEvent.touches.length > 1) track(e);
        else
          hold.current = setTimeout(() => {
            gesture.current.moved = true;
            tap(x, y, true);
          }, 550);
      }}
      onResponderMove={track}
      onResponderTerminate={clearHold}
      onResponderRelease={(e) => {
        clearHold();
        if (!gesture.current.moved)
          tap(e.nativeEvent.locationX, e.nativeEvent.locationY, false);
      }}
    >
      {uri ? (
        <Image
          accessible={false}
          pointerEvents="none"
          resizeMode="stretch"
          source={{ uri }}
          style={{
            position: 'absolute',
            left: transform.left,
            top: transform.top,
            width: viewport.width * transform.zoom,
            height: viewport.height * transform.zoom,
          }}
        />
      ) : null}
    </View>
  );
}
