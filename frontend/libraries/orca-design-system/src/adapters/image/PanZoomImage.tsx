import { useEffect, useRef, useState } from 'react';
import { Image, View, type GestureResponderEvent } from 'react-native';
import {
  containPanZoomView,
  getFittedPanZoomView,
  getPanZoomFitScale,
  getPanZoomViewOffset,
  panView,
  zoomViewAt,
  type PanZoomPoint,
  type PanZoomSize,
  type PanZoomView,
} from './panZoomView';

function points(event: GestureResponderEvent) {
  return event.nativeEvent.touches.map(({ pageX: x, pageY: y }) => ({ x, y }));
}
const distance = (a: PanZoomPoint, b: PanZoomPoint) =>
  Math.hypot(b.x - a.x, b.y - a.y);
const middle = (a: PanZoomPoint, b: PanZoomPoint) => ({
  x: (a.x + b.x) / 2,
  y: (a.y + b.y) / 2,
});

export function PanZoomImage({ uri, label }: { uri: string; label: string }) {
  const [content, setContent] = useState<PanZoomSize>();
  const [viewport, setViewport] = useState<PanZoomSize>();
  const [manual, setManual] = useState<PanZoomView>();
  const previous = useRef<PanZoomPoint[]>([]);
  const touchStart = useRef<PanZoomPoint | null>(null);
  const lastTap = useRef<{ point: PanZoomPoint; at: number } | null>(null);
  useEffect(() => {
    let active = true;
    Image.getSize(uri).then(
      (size) => {
        if (active) setContent(size);
      },
      () => {}
    );
    return () => {
      active = false;
    };
  }, [uri]);
  const geometry =
    content && viewport
      ? { content, viewport, fitScale: getPanZoomFitScale(content, viewport) }
      : null;
  const view =
    geometry &&
    (manual
      ? containPanZoomView(manual, geometry)
      : getFittedPanZoomView(geometry.content));
  const scale = geometry && view ? geometry.fitScale * view.zoom : 0;
  const offset =
    viewport && view ? getPanZoomViewOffset(view, viewport, scale) : null;
  function move(event: GestureResponderEvent) {
    const current = points(event);
    const prior = previous.current;
    previous.current = current;
    if (!geometry || !current.length || !prior.length) return;
    if (
      current.length > 1 ||
      (touchStart.current && distance(touchStart.current, current[0]) > 10)
    )
      touchStart.current = null;
    if (current.length !== prior.length) return;
    setManual((value) => {
      const from = value ?? getFittedPanZoomView(geometry.content);
      if (current.length >= 2)
        return zoomViewAt({
          ...geometry,
          view: from,
          nextZoom:
            (from.zoom * distance(current[0], current[1])) /
            Math.max(1, distance(prior[0], prior[1])),
          anchor: middle(prior[0], prior[1]),
          target: middle(current[0], current[1]),
          contain: true,
        });
      return panView(
        from,
        { x: current[0].x - prior[0].x, y: current[0].y - prior[0].y },
        geometry.fitScale * from.zoom,
        geometry.content,
        geometry.viewport
      );
    });
  }
  function end() {
    const point = touchStart.current;
    if (geometry && view && point) {
      const last = lastTap.current;
      if (
        last &&
        Date.now() - last.at <= 300 &&
        distance(last.point, point) <= 10
      ) {
        setManual(
          view.zoom === 1
            ? zoomViewAt({
                ...geometry,
                view,
                nextZoom: 2.5,
                anchor: point,
                contain: true,
              })
            : undefined
        );
        lastTap.current = null;
      } else lastTap.current = { at: Date.now(), point };
    }
    previous.current = [];
    touchStart.current = null;
  }
  return (
    <View
      style={{ flex: 1, overflow: 'hidden' }}
      onLayout={({ nativeEvent: { layout } }) => setViewport(layout)}
      onStartShouldSetResponder={() => true}
      onResponderGrant={(event) => {
        previous.current = points(event);
        touchStart.current = previous.current[0];
      }}
      onResponderMove={move}
      onResponderRelease={end}
      onResponderTerminate={() => {
        previous.current = [];
        touchStart.current = null;
      }}
    >
      {offset && content && (
        <Image
          accessible
          accessibilityLabel={label}
          source={{ uri }}
          style={{
            position: 'absolute',
            left: offset.x,
            top: offset.y,
            width: content.width * scale,
            height: content.height * scale,
          }}
        />
      )}
    </View>
  );
}
