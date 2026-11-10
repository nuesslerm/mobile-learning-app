import { useState } from 'react';
import { Text, View } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import { describe, expect, it } from 'vitest';

import { createFlowApp } from '../flowApp';
import { screen } from '../index';

// A Pan handler that records every pointer position (x,y relative to its own
// box, which the layout driver places at the page origin) into the DOM, so the
// test asserts the exact samples a session.stroke produced.
function StrokeCanvas() {
  const [points, setPoints] = useState<string[]>([]);
  const pan = Gesture.Pan()
    .onBegin((e) =>
      setPoints((log) => [...log, `${Math.round(e.x)},${Math.round(e.y)}`])
    )
    .onStart((e) =>
      setPoints((log) => [...log, `${Math.round(e.x)},${Math.round(e.y)}`])
    )
    .onUpdate((e) =>
      setPoints((log) => [...log, `${Math.round(e.x)},${Math.round(e.y)}`])
    )
    .onEnd((e) =>
      setPoints((log) => [...log, `${Math.round(e.x)},${Math.round(e.y)}`])
    );
  return (
    <GestureDetector gesture={pan}>
      <View testID="canvas" style={{ width: 200, height: 100 }}>
        <Text testID="canvas-log">{points.join(' | ')}</Text>
      </View>
    </GestureDetector>
  );
}

const app = createFlowApp({
  routes: { '../routes/index.tsx': { default: StrokeCanvas } },
  routesRoot: '../routes/',
});

function recordedPoints(): number[][] {
  const log = screen.getByTestId('canvas-log').textContent ?? '';
  return log
    .split(' | ')
    .filter(Boolean)
    .map((entry) => entry.split(',').map(Number));
}

describe('session stroke', () => {
  it('given a laid-out Pan canvas, when a fraction-unit stroke is drawn, then the handler records the first point at 0,0 and the last at 200,100', async () => {
    const session = await app.render('/');

    await session.layout('canvas', { width: 200, height: 100 });
    const result = await session.stroke(
      'canvas',
      [
        { x: 0, y: 0 },
        { x: 1, y: 1 },
      ],
      { samplesPerSegment: 4 }
    );

    const recorded = recordedPoints();
    expect(result.samples).toBe(5);
    expect(recorded[0]).toEqual([0, 0]);
    expect(recorded[recorded.length - 1]).toEqual([200, 100]);
  });

  it('given a reverse transform, when a stroke is drawn, then the first recorded point is the transformed end at 200,100', async () => {
    const session = await app.render('/');

    await session.layout('canvas', { width: 200, height: 100 });
    await session.stroke(
      'canvas',
      [
        { x: 0, y: 0 },
        { x: 1, y: 1 },
      ],
      { samplesPerSegment: 4, transform: { reverse: true } }
    );

    const recorded = recordedPoints();
    expect(recorded[0]).toEqual([200, 100]);
    expect(recorded[recorded.length - 1]).toEqual([0, 0]);
  });
});
