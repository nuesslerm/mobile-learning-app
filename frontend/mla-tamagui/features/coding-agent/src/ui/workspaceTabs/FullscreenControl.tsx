import { useMemo, useRef, useState } from 'react';
import { PanResponder, Pressable, View } from 'react-native';
import { Maximize2, Minimize2 } from 'lucide-react-native';

type Props = {
  fullscreen: boolean;
  visible: boolean;
  viewport: { width: number; height: number };
  hold: () => void;
  release: () => void;
  toggle: () => void;
};
const diameter = 44;
const inset = 12;
const clamp = (n: number) => Math.max(0, Math.min(1, n));

export function FullscreenControl(props: Props) {
  const [position, setPosition] = useState({ x: 1, y: 1 });
  const spanX = Math.max(0, props.viewport.width - diameter - 2 * inset);
  const spanY = Math.max(0, props.viewport.height - diameter - 2 * inset);
  const latest = useRef({ props, position, spanX, spanY });
  latest.current = { props, position, spanX, spanY };
  const start = useRef(position);
  const pan = useMemo(
    () =>
      PanResponder.create({
        onMoveShouldSetPanResponderCapture: (_, g) =>
          Math.hypot(g.dx, g.dy) > 5,
        onPanResponderGrant: () => {
          start.current = latest.current.position;
          latest.current.props.hold();
        },
        onPanResponderMove: (_, g) => {
          latest.current.props.hold();
          setPosition({
            x: clamp(
              start.current.x + g.dx / Math.max(1, latest.current.spanX)
            ),
            y: clamp(
              start.current.y + g.dy / Math.max(1, latest.current.spanY)
            ),
          });
        },
        onPanResponderTerminationRequest: () => false,
        onPanResponderRelease: () => latest.current.props.release(),
        onPanResponderTerminate: () => latest.current.props.release(),
      }),
    []
  );
  if (!props.visible) return null;
  return (
    <View
      {...pan.panHandlers}
      style={{
        position: 'absolute',
        left: inset + spanX * position.x,
        top: inset + spanY * position.y,
      }}
    >
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={
          props.fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'
        }
        accessibilityHint="Drag to move the button"
        onPressIn={props.hold}
        onPressOut={props.release}
        onPress={props.toggle}
        style={{
          width: diameter,
          height: diameter,
          borderRadius: diameter / 2,
          backgroundColor: 'rgba(0,0,0,0.74)',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        {props.fullscreen ? (
          <Minimize2 color="white" size={19} />
        ) : (
          <Maximize2 color="white" size={19} />
        )}
      </Pressable>
    </View>
  );
}
