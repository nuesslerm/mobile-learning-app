import { LoadSkiaWeb, WithSkiaWeb } from '@/lib/skiaWeb';
import {
  Canvas,
  Circle,
  Path,
  Skia,
  type SkPath,
} from '@shopify/react-native-skia';
import { Text, View } from '@lib/tamagui-design-system';
import {
  type ComponentProps,
  createContext,
  useContext,
  useCallback,
  useState,
  useSyncExternalStore,
} from 'react';
import { Platform, type LayoutChangeEvent } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import { useTheme } from '@lib/tamagui-design-system/primitives';

import type { GlyphContour } from '../domain/glyphGeometry';
import {
  appendPoint,
  pointsToPath,
  pointsToSmoothPath,
  type CanvasSize,
  type Point,
  type Stroke,
} from '../domain/strokes';

import { GuideGlyph } from './GuideGlyph';
import type { TracingCanvasProps } from './TracingCanvasSvg';

const DEFAULT_HEIGHT = 300;
const STROKE_WIDTH = 8;
const GUIDE_WIDTH = 10;
const ARROW = 11;
// Tamil letters are wide -- ஔ and கௌ run nearly twice the width of அ -- so the
// guide is sized off the pad's shorter side with room to spare. At 0.62 the
// widest glyphs were clipped by the pad's own `overflow: hidden`.
const GUIDE_SCALE = 0.45;

// The web path is a CanvasKit WASM bundle loaded by LoadSkiaWeb, the same way
// the charts-spike probe does: kick the fetch off at module scope on the
// client, and let WithSkiaWeb's lazy loader await the shared promise. The SSR
// pass runs in Node, so it never calls LoadSkiaWeb and renders the fallback.
const SKIA_WASM_BASE =
  'https://cdn.jsdelivr.net/npm/canvaskit-wasm@0.41.0/bin/full';

// `window` also exists on React Native, where LoadSkiaWeb is a stub that throws.
if (Platform.OS === 'web' && typeof window !== 'undefined') {
  void LoadSkiaWeb({
    locateFile: (file) => `${SKIA_WASM_BASE}/${file}`,
  });
}

const FALLBACK = (
  <Text p="$4" testID="TamilTracingCanvasLoading">
    Loading Skia…
  </Text>
);

/** Never fires: the value flips once, when the client takes over rendering. */
const subscribeNever = () => () => {};

function useHydrated(): boolean {
  return useSyncExternalStore(
    subscribeNever,
    () => true,
    () => false
  );
}

/** The flow harness's WithSkiaWeb renders the loaded component without
 * componentProps, so the body reads the props from context, which the wrapper
 * always provides. */
const TracingCanvasPropsContext = createContext<TracingCanvasProps | null>(null);

/** Skia types carry no testID; the flow harness's stub reads it off the props,
 * and the real renderers spread extra props through harmlessly. */
type TestablePathProps = ComponentProps<typeof Path> & { testID?: string };
type TestableCircleProps = ComponentProps<typeof Circle> & { testID?: string };

function TestablePath(props: TestablePathProps): React.ReactElement {
  return <Path {...props} />;
}

function TestableCircle(props: TestableCircleProps): React.ReactElement {
  return <Circle {...props} />;
}

/** MakeFromSVGString returns null for a malformed path; ours are built from
 * points, so a null is a bug worth throwing over. */
function makePath(d: string): SkPath {
  const path = Skia.Path.MakeFromSVGString(d);
  if (!path) {
    throw new Error(`TracingCanvasSkia: invalid path ${d}`);
  }
  return path;
}

/**
 * The writing pad drawn by Skia instead of SVG: the same props, the same
 * gesture, the same data attributes on the surface. On web the canvas is
 * mounted behind WithSkiaWeb so CanvasKit can load; the Canvas itself renders
 * the outline, guide, active guide, start dot and ink as paths.
 */
export function TracingCanvasSkia(props: TracingCanvasProps): React.ReactElement {
  const hydrated = useHydrated();

  return (
    <TracingCanvasPropsContext.Provider value={props}>
      {Platform.OS === 'web' ? (
        hydrated ? (
          <WithSkiaWeb getComponent={getSkiaCanvas} fallback={FALLBACK} />
        ) : (
          FALLBACK
        )
      ) : (
        <SkiaTracingCanvas />
      )}
    </TracingCanvasPropsContext.Provider>
  );
}

const getSkiaCanvas = (): Promise<{ default: typeof SkiaTracingCanvas }> =>
  Promise.resolve({ default: SkiaTracingCanvas });

function SkiaTracingCanvas(): React.ReactElement {
  const props = useContext(TracingCanvasPropsContext);
  if (!props) {
    throw new Error('TracingCanvasSkia: rendered without a props provider');
  }
  const {
    glyph,
    strokes,
    onStroke,
    onSizeChange,
    showGuide = true,
    completedGuides,
    activeGuide,
    outline,
    height = DEFAULT_HEIGHT,
    templateScale = GUIDE_SCALE,
    testID = 'TamilTracingCanvas',
  } = props;

  const theme = useTheme();
  const inkColor = theme.blue9?.val ?? '#0091ff';
  const guideColor = theme.green9?.val ?? '#30a46c';
  const [size, setSize] = useState<CanvasSize>({
    width: 0,
    height: height === 'fill' ? 0 : height,
  });
  const [current, setCurrent] = useState<Stroke | null>(null);

  const handleLayout = useCallback(
    (event: LayoutChangeEvent) => {
      const next = {
        width: event.nativeEvent.layout.width,
        height: event.nativeEvent.layout.height,
      };
      setSize(next);
      onSizeChange?.(next);
    },
    [onSizeChange]
  );

  const beginStroke = useCallback((point: Point) => {
    setCurrent([point]);
  }, []);

  const extendStroke = useCallback((point: Point) => {
    setCurrent((stroke) => (stroke ? appendPoint(stroke, point) : [point]));
  }, []);

  const endStroke = useCallback(() => {
    setCurrent((stroke) => {
      if (stroke && stroke.length > 0) onStroke(stroke);
      return null;
    });
  }, [onStroke]);

  // runOnJS(true) keeps every callback on the JS thread: the stroke lives in
  // React state, and a worklet cannot call setState.
  const pan = Gesture.Pan()
    .runOnJS(true)
    .minDistance(0)
    .maxPointers(1)
    .onBegin((event) => beginStroke({ x: event.x, y: event.y }))
    .onUpdate((event) => extendStroke({ x: event.x, y: event.y }))
    .onEnd(endStroke)
    .onFinalize(endStroke);


  /** Reference paths are pad fractions; the pad is pixels. */
  const toPad = (point: Point): Point => ({
    x: point.x * size.width,
    y: point.y * size.height,
  });

  const activePath = activeGuide?.points.map(toPad) ?? [];
  const activeStart = activePath[0];
  const activeEnd = activePath[activePath.length - 1];
  const activePrevious = activePath[activePath.length - 2];

  return (
    <View
      testID={testID}
      {...(height === 'fill' ? { flex: 1 } : { height })}
      rounded="$4"
      overflow="hidden"
      borderWidth={1}
      borderColor="$borderColor"
      bg="$color2"
      onLayout={handleLayout}
    >
      {showGuide && !outline ? (
        <GuideGlyph
          glyph={glyph}
          width={size.width}
          height={size.height}
          scale={templateScale}
          testID={testID}
        />
      ) : null}

      <GestureDetector gesture={pan}>
        <View
          flex={1}
          testID={`${testID}Surface`}
          data-active-guide={
            activePath.length > 1 ? pointsToPath(activePath) : ''
          }
          data-ink-count={strokes.length}
        >
          <Canvas>
            {showGuide && outline ? (
              <TestablePath
                testID={`${testID}Outline`}
                path={makePath(outlinePath(outline, size))}
                color={theme.color?.val ?? '#000'}
                style="fill"
                fillType="winding"
                opacity={0.12}
              />
            ) : null}

            {(completedGuides ?? []).map((stroke, index) => (
              <TestablePath
                key={`done-${index}`}
                // The polyline, not a smoothed curve: this is the line the
                // stroke is marked against, and rounding its corners for
                // display made tracing it exactly score 67% on ட.
                path={makePath(pointsToPath(stroke.points.map(toPad)))}
                color={guideColor}
                strokeWidth={GUIDE_WIDTH}
                style="stroke"
                strokeCap="round"
                strokeJoin="round"
                opacity={0.15}
              />
            ))}

            {activePath.length > 1 ? (
              <TestablePath
                testID={`${testID}ActiveGuide`}
                path={makePath(pointsToPath(activePath))}
                color={guideColor}
                strokeWidth={GUIDE_WIDTH}
                style="stroke"
                strokeCap="round"
                strokeJoin="round"
                opacity={0.35}
              />
            ) : null}
            {activeStart ? (
              <TestableCircle
                testID={`${testID}StartDot`}
                cx={activeStart.x}
                cy={activeStart.y}
                r={7}
                color={guideColor}
              />
            ) : null}
            {activeEnd && activePrevious ? (
              <TestablePath
                testID={`${testID}DirectionArrow`}
                path={makePath(arrowPath(activePrevious, activeEnd))}
                color={guideColor}
                style="fill"
              />
            ) : null}

            {strokes.map((stroke, index) => (
              <TestablePath
                key={index}
                path={makePath(pointsToSmoothPath(stroke))}
                color={inkColor}
                strokeWidth={STROKE_WIDTH}
                style="stroke"
                strokeCap="round"
                strokeJoin="round"
              />
            ))}
            {current ? (
              <TestablePath
                path={makePath(pointsToSmoothPath(current))}
                color={inkColor}
                strokeWidth={STROKE_WIDTH}
                style="stroke"
                strokeCap="round"
                strokeJoin="round"
              />
            ) : null}
          </Canvas>
        </View>
      </GestureDetector>
    </View>
  );
}

/** The glyph's rings as one even-odd path, scaled into the pad. */
function outlinePath(
  contours: readonly GlyphContour[],
  size: CanvasSize
): string {
  return contours
    .map((contour) => {
      const points = contour.map(
        (point) =>
          `${(point.x * size.width).toFixed(1)} ${(point.y * size.height).toFixed(1)}`
      );
      return points.length > 0 ? `M ${points.join(' L ')} Z` : '';
    })
    .filter(Boolean)
    .join(' ');
}

/** A triangle at `tip`, pointing the way the stroke was travelling. */
function arrowPoints(from: Point, tip: Point): string {
  const angle = Math.atan2(tip.y - from.y, tip.x - from.x);
  const corner = (offset: number): string => {
    const a = angle + offset;
    return `${tip.x - Math.cos(a) * ARROW},${tip.y - Math.sin(a) * ARROW}`;
  };
  return `${tip.x},${tip.y} ${corner(0.5)} ${corner(-0.5)}`;
}

/** The direction arrow as a fillable path (Skia has no Polygon primitive). */
function arrowPath(from: Point, tip: Point): string {
  const [tipPoint, left, right] = arrowPoints(from, tip)
    .split(' ')
    .map((pair) => pair.split(','));
  if (!tipPoint || !left || !right) return '';
  return `M ${tipPoint[0]} ${tipPoint[1]} L ${left[0]} ${left[1]} L ${right[0]} ${right[1]} Z`;
}