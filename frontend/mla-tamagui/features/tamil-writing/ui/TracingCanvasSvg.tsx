import { View } from '@lib/tamagui-design-system';
import { useCallback, useEffect, useReducer, useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Svg, { Circle, Path, Polygon } from 'react-native-svg';
import { useTheme } from '@lib/tamagui-design-system/primitives';

import type { GlyphContour } from '../domain/glyphGeometry';
import type { ReferenceStroke } from '../domain/strokeGuide';
import {
  appendPoint,
  pointsToPath,
  pointsToSmoothPath,
  type CanvasSize,
  type Point,
  type Stroke,
} from '../domain/strokes';

import { GuideGlyph } from './GuideGlyph';

export type TracingCanvasProps = {
  /** The letter shown faintly underneath, for the finger to follow. */
  glyph: string;
  /** Strokes to render. The parent owns them, so it can refuse one. */
  strokes: readonly Stroke[];
  /** A finished stroke. The parent decides whether to keep it. */
  onStroke: (stroke: Stroke) => void;
  onSizeChange?: (size: CanvasSize) => void;
  showGuide?: boolean;
  /** Reference strokes already written, drawn faintly behind the ink. */
  completedGuides?: readonly ReferenceStroke[];
  /** The stroke being asked for: drawn as a line with a start dot and an arrow. */
  activeGuide?: ReferenceStroke;
  /** The letter's own outline. Filled instead of drawing the glyph as text, so
   * the printed letter and the stroke guide are the same geometry. */
  outline?: readonly GlyphContour[];
  /** Pixels, or `'fill'` to take whatever height the parent gives. */
  height?: number | 'fill';
  /**
   * How much of the pad's shorter side the printed glyph takes. The stroke
   * coach keeps it small so the guide lines fit around it; the write-large
   * pad wants the letter to fill the page.
   */
  templateScale?: number;
  testID?: string;
};

const DEFAULT_HEIGHT = 300;
const STROKE_WIDTH = 8;
const GUIDE_WIDTH = 10;
const ARROW = 11;
// Tamil letters are wide -- ஔ and கௌ run nearly twice the width of அ -- so the
// guide is sized off the pad's shorter side with room to spare. At 0.62 the
// widest glyphs were clipped by the pad's own `overflow: hidden`.
const GUIDE_SCALE = 0.45;

/**
 * The writing pad: a faint guide letter, the stroke being asked for drawn over
 * it, and the learner's ink on top.
 *
 * The glyph is the font's own rather than traced outline data -- Tamil ships
 * with the system on both platforms, so a 247-letter course needs no vector
 * assets. The reference strokes on top are what the validator scores against,
 * so what is shown and what is marked are the same line.
 *
 * The in-progress stroke is local state and only reaches the parent on
 * release: lifting every finger sample would re-render the whole screen at
 * touch rate.
 */
type StrokeState = { current: Stroke | null; done: Stroke | null };
type StrokeAction = { type: 'begin' | 'extend'; point: Point } | { type: 'end' } | { type: 'handed-over' };

function strokeReducer(state: StrokeState, action: StrokeAction): StrokeState {
  switch (action.type) {
    case 'begin':
      return { ...state, current: [action.point] };
    case 'extend':
      return { ...state, current: state.current ? appendPoint(state.current, action.point) : [action.point] };
    case 'end':
      return state.current === null
        ? state
        : { current: null, done: state.current.length > 0 ? state.current : state.done };
    case 'handed-over':
      return { ...state, done: null };
  }
}

export function TracingCanvasSvg({
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
}: TracingCanvasProps): React.ReactElement {
  const theme = useTheme();
  const inkColor = theme.blue9?.val ?? '#0091ff';
  const guideColor = theme.green9?.val ?? '#30a46c';
  const [size, setSize] = useState<CanvasSize>({
    width: 0,
    height: height === 'fill' ? 0 : height,
  });
  // A finished stroke is handed to `onStroke` from an effect, never from the reducer: calling the parent while this
  // component renders makes React report an update of the screen during render.
  const [{ current, done }, dispatch] = useReducer(strokeReducer, { current: null, done: null });
  useEffect(() => {
    if (!done) return;
    onStroke(done);
    dispatch({ type: 'handed-over' });
  }, [done, onStroke]);

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

  const beginStroke = useCallback((point: Point) => dispatch({ type: 'begin', point }), []);
  const extendStroke = useCallback((point: Point) => dispatch({ type: 'extend', point }), []);
  const endStroke = useCallback(() => dispatch({ type: 'end' }), []);

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
          <Svg width="100%" height="100%">
            {showGuide && outline ? (
              <Path
                testID={`${testID}Outline`}
                d={outlinePath(outline, size)}
                fill={theme.color?.val ?? '#000'}
                // Nonzero, like the font: these contours overlap, and even-odd
                // turns every overlap into a hole through the letter.
                fillRule="nonzero"
                opacity={0.12}
              />
            ) : null}

            {(completedGuides ?? []).map((stroke, index) => (
              <Path
                key={`done-${index}`}
                // The polyline, not a smoothed curve: this is the line the
                // stroke is marked against, and rounding its corners for
                // display made tracing it exactly score 67% on ட.
                d={pointsToPath(stroke.points.map(toPad))}
                stroke={guideColor}
                strokeWidth={GUIDE_WIDTH}
                strokeLinecap="round"
                strokeLinejoin="round"
                fill="none"
                opacity={0.15}
              />
            ))}

            {activePath.length > 1 ? (
              <Path
                testID={`${testID}ActiveGuide`}
                d={pointsToPath(activePath)}
                stroke={guideColor}
                strokeWidth={GUIDE_WIDTH}
                strokeLinecap="round"
                strokeLinejoin="round"
                fill="none"
                opacity={0.35}
              />
            ) : null}
            {activeStart ? (
              <Circle
                testID={`${testID}StartDot`}
                cx={activeStart.x}
                cy={activeStart.y}
                r={7}
                fill={guideColor}
              />
            ) : null}
            {activeEnd && activePrevious ? (
              <Polygon
                testID={`${testID}DirectionArrow`}
                points={arrowPoints(activePrevious, activeEnd)}
                fill={guideColor}
              />
            ) : null}

            {strokes.map((stroke, index) => (
              <Path
                key={index}
                d={pointsToSmoothPath(stroke)}
                stroke={inkColor}
                strokeWidth={STROKE_WIDTH}
                strokeLinecap="round"
                strokeLinejoin="round"
                fill="none"
              />
            ))}
            {current ? (
              <Path
                d={pointsToSmoothPath(current)}
                stroke={inkColor}
                strokeWidth={STROKE_WIDTH}
                strokeLinecap="round"
                strokeLinejoin="round"
                fill="none"
              />
            ) : null}
          </Svg>
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
