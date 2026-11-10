/**
 * AnimatedWaveform — row of 14 vertical bars that animate while the user is
 * recording voice commands. Sits inside the VoiceRecordingToolbar or any other
 * surface that needs visible "I'm listening" feedback.
 *
 * Passive mode: a single phase value drives a sinusoidal wave across the bars
 * (deterministic per-bar offset). Active mode: heights are fixed to the
 * supplied amplitudes. Both run on the UI thread via reanimated.
 */
import * as React from 'react';
import Animated, {
  cancelAnimation,
  useAnimatedStyle,
  useSharedValue,
  withRepeat,
  withTiming,
  type SharedValue,
} from 'react-native-reanimated';
import { useTheme } from '@lib/tamagui-design-system/primitives';

const BAR_COUNT = 14;
const BAR_WIDTH = 3;
const BAR_GAP = 3;
const MIN_HEIGHT = 6;
const MAX_HEIGHT = 28;

const PHASE_DURATION = 1200;

type AnimatedWaveformProps = {
  isRecording: boolean;
  /** Optional 0–1 amplitudes; when supplied, bars are set to those values. */
  amplitudes?: number[];
  testID?: string;
};

function seededPhase(index: number): number {
  'worklet';
  return ((index * 37) % 360) * (Math.PI / 180);
}

export function AnimatedWaveform({
  isRecording,
  amplitudes,
  testID = 'AnimatedWaveform',
}: AnimatedWaveformProps) {
  const phase = useSharedValue(0);
  // Reanimated styles cannot resolve Tamagui tokens, so the bar colour is read
  // out of the theme here and passed down as a plain string.
  const barColor = useTheme().blue9?.val ?? '#0091ff';
  const latestAmps = React.useRef<number[] | undefined>(amplitudes);
  latestAmps.current = amplitudes;

  // One effect, not the two app #1 carries: the first was a strict subset of
  // this one, so it only ever re-ran the same animation an extra time.
  React.useEffect(() => {
    cancelAnimation(phase);
    if (!isRecording) {
      phase.value = 0;
    } else if (!amplitudes || amplitudes.length === 0) {
      phase.value = withRepeat(
        withTiming(2 * Math.PI, { duration: PHASE_DURATION }),
        -1,
        false
      );
    }
  }, [amplitudes, isRecording, phase]);

  return (
    <Animated.View
      testID={testID}
      aria-label="Voice waveform"
      style={{
        height: MAX_HEIGHT,
        gap: BAR_GAP,
        flexDirection: 'row',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      {Array.from({ length: BAR_COUNT }, (_, idx) => {
        const barTestID = `${testID}-Bar-${idx}`;
        return (
          <Bar
            key={idx}
            phase={phase}
            index={idx}
            amplitudes={latestAmps.current}
            color={barColor}
            testID={barTestID}
          />
        );
      })}
    </Animated.View>
  );
}

function Bar({
  phase,
  index,
  amplitudes,
  color,
  testID,
}: {
  phase: SharedValue<number>;
  index: number;
  amplitudes?: number[];
  color: string;
  testID: string;
}) {
  const animatedStyle = useAnimatedStyle(() => {
    const amp = amplitudes
      ? clamp01(amplitudes[index] ?? amplitudes[amplitudes.length - 1] ?? 0)
      : undefined;
    const scale =
      amp !== undefined
        ? amp
        : 0.35 + 0.35 * Math.sin(phase.value + seededPhase(index));
    // Colour and radius live in the SAME animated style object rather than a
    // second static one: `[static, animated]` style arrays on an Animated
    // component are banned repo-wide.
    return {
      height: MIN_HEIGHT + scale * (MAX_HEIGHT - MIN_HEIGHT),
      width: BAR_WIDTH,
      borderRadius: BAR_WIDTH / 2,
      backgroundColor: color,
    };
  });
  return <Animated.View testID={testID} style={animatedStyle} />;
}

function clamp01(value: number): number {
  'worklet';
  return Math.min(1, Math.max(0, value));
}
