import { fireEvent, render } from '@testing-library/react';
import { useState } from 'react';
import { Text, View } from 'react-native';
import {
  Gesture,
  GestureDetector,
  type GestureType,
} from 'react-native-gesture-handler';
import { describe, expect, it } from 'vitest';

import { screen } from '../index';
import { setLayout } from '../interactions/layout';

// Every callback name and payload is serialized into the DOM (testID-log), so a
// test drives pointer events and asserts the exact lifecycle the stub fired.
// The gestures are typed loosely: at runtime they are the stub's plain objects
// (the real package's union types cannot host a common callback signature).
type GestureLike = {
  onBegin(fn: (e: unknown, success?: boolean) => void): GestureLike;
  onStart(fn: (e: unknown, success?: boolean) => void): GestureLike;
  onUpdate(fn: (e: unknown, success?: boolean) => void): GestureLike;
  onChange(fn: (e: unknown, success?: boolean) => void): GestureLike;
  onEnd(fn: (e: unknown, success?: boolean) => void): GestureLike;
  onFinalize(fn: (e: unknown, success?: boolean) => void): GestureLike;
};

type Recorder = (
  tag: string,
  name: string,
  e: unknown,
  success?: boolean
) => void;

function rec(tag: string, name: string, e: unknown, success?: boolean): string {
  const ev = (e ?? {}) as Record<string, number | undefined>;
  const num = (v: number | undefined) => (typeof v === 'number' ? String(v) : '?');
  const parts = [
    `x=${num(ev.x)}`,
    `y=${num(ev.y)}`,
    `tx=${num(ev.translationX)}`,
    `ty=${num(ev.translationY)}`,
    `cx=${num(ev.changeX)}`,
    `cy=${num(ev.changeY)}`,
  ];
  if (success !== undefined) parts.push(`success=${success}`);
  return `${tag}.${name}(${parts.join(',')})`;
}

function attachHandlers(g: unknown, tag: string, record: Recorder): void {
  const gesture = g as GestureLike;
  gesture.onBegin((e, success) => record(tag, 'onBegin', e, success));
  gesture.onStart((e, success) => record(tag, 'onStart', e, success));
  gesture.onUpdate((e, success) => record(tag, 'onUpdate', e, success));
  gesture.onChange((e, success) => record(tag, 'onChange', e, success));
  gesture.onEnd((e, success) => record(tag, 'onEnd', e, success));
  gesture.onFinalize((e, success) => record(tag, 'onFinalize', e, success));
}

function GestureProbe({
  build,
  testID,
}: {
  build: (record: Recorder) => unknown;
  testID: string;
}) {
  const [log, setLog] = useState<string[]>([]);
  const record: Recorder = (tag, name, e, success) =>
    setLog((lines) => [...lines, rec(tag, name, e, success)]);
  return (
    <GestureDetector gesture={build(record) as GestureType}>
      <View testID={testID} style={{ width: 200, height: 100 }}>
        <Text testID={`${testID}-log`}>{log.join(' | ')}</Text>
      </View>
    </GestureDetector>
  );
}

function giveBox(testID: string): void {
  setLayout(screen.getByTestId(testID), {
    x: 100,
    y: 50,
    width: 200,
    height: 100,
  });
}

function logOf(testID: string): string {
  return screen.getByTestId(`${testID}-log`).textContent ?? '';
}

describe('gesture stub lifecycle', () => {
  it('given a Pan, when a drag is driven, then onBegin fires on pointerdown and onStart/onUpdate/onChange follow, ending in onEnd and onFinalize with success true', () => {
    render(
      <GestureProbe
        testID="pan"
        build={(record) => {
          const pan = Gesture.Pan();
          attachHandlers(pan, 'pan', record);
          return pan;
        }}
      />
    );
    giveBox('pan');
    const target = screen.getByTestId('pan');

    fireEvent.pointerDown(target, { clientX: 120, clientY: 60 });
    fireEvent.pointerMove(target, { clientX: 130, clientY: 60 });
    fireEvent.pointerMove(target, { clientX: 150, clientY: 60 });
    fireEvent.pointerUp(target, { clientX: 160, clientY: 60 });

    expect(logOf('pan')).toBe(
      [
        'pan.onBegin(x=20,y=10,tx=0,ty=0,cx=0,cy=0)',
        'pan.onStart(x=30,y=10,tx=10,ty=0,cx=10,cy=0)',
        'pan.onUpdate(x=30,y=10,tx=10,ty=0,cx=10,cy=0)',
        'pan.onChange(x=30,y=10,tx=10,ty=0,cx=10,cy=0)',
        'pan.onUpdate(x=50,y=10,tx=30,ty=0,cx=20,cy=0)',
        'pan.onChange(x=50,y=10,tx=30,ty=0,cx=20,cy=0)',
        'pan.onEnd(x=60,y=10,tx=40,ty=0,cx=10,cy=0,success=true)',
        'pan.onFinalize(x=60,y=10,tx=40,ty=0,cx=10,cy=0,success=true)',
      ].join(' | ')
    );
  });

  it('given a Pan with minDistance 20, when it is moved only 5px, then it never activates: onFinalize runs with success false and no onStart/onEnd', () => {
    render(
      <GestureProbe
        testID="pan-min"
        build={(record) => {
          const pan = Gesture.Pan().minDistance(20);
          attachHandlers(pan, 'pan', record);
          return pan;
        }}
      />
    );
    giveBox('pan-min');
    const target = screen.getByTestId('pan-min');

    fireEvent.pointerDown(target, { clientX: 120, clientY: 60 });
    fireEvent.pointerMove(target, { clientX: 123, clientY: 60 });
    fireEvent.pointerMove(target, { clientX: 125, clientY: 60 });
    fireEvent.pointerUp(target, { clientX: 125, clientY: 60 });

    expect(logOf('pan-min')).toBe(
      [
        'pan.onBegin(x=20,y=10,tx=0,ty=0,cx=0,cy=0)',
        'pan.onFinalize(x=25,y=10,tx=5,ty=0,cx=0,cy=0,success=false)',
      ].join(' | ')
    );
  });

  it('given a Tap, when it is released within 10px, then onBegin, onStart, onEnd and onFinalize all fire with success true', () => {
    render(
      <GestureProbe
        testID="tap"
        build={(record) => {
          const tap = Gesture.Tap();
          attachHandlers(tap, 'tap', record);
          return tap;
        }}
      />
    );
    giveBox('tap');
    const target = screen.getByTestId('tap');

    fireEvent.pointerDown(target, { clientX: 120, clientY: 60 });
    fireEvent.pointerUp(target, { clientX: 121, clientY: 61 });

    expect(logOf('tap')).toBe(
      [
        'tap.onBegin(x=20,y=10,tx=0,ty=0,cx=0,cy=0)',
        'tap.onStart(x=21,y=11,tx=1,ty=1,cx=1,cy=1)',
        'tap.onEnd(x=21,y=11,tx=1,ty=1,cx=1,cy=1,success=true)',
        'tap.onFinalize(x=21,y=11,tx=1,ty=1,cx=1,cy=1,success=true)',
      ].join(' | ')
    );
  });

  it('given a Race(pan, tap), when it is tapped, then the tap activates and the pan only finalizes with success false', () => {
    render(
      <GestureProbe
        testID="race-tap"
        build={(record) => {
          const pan = Gesture.Pan();
          const tap = Gesture.Tap();
          attachHandlers(pan, 'pan', record);
          attachHandlers(tap, 'tap', record);
          return Gesture.Race(pan, tap);
        }}
      />
    );
    giveBox('race-tap');
    const target = screen.getByTestId('race-tap');

    fireEvent.pointerDown(target, { clientX: 120, clientY: 60 });
    fireEvent.pointerUp(target, { clientX: 121, clientY: 61 });

    expect(logOf('race-tap')).toBe(
      [
        'pan.onBegin(x=20,y=10,tx=0,ty=0,cx=0,cy=0)',
        'tap.onBegin(x=20,y=10,tx=0,ty=0,cx=0,cy=0)',
        'pan.onFinalize(x=21,y=11,tx=1,ty=1,cx=1,cy=1,success=false)',
        'tap.onStart(x=21,y=11,tx=1,ty=1,cx=1,cy=1)',
        'tap.onEnd(x=21,y=11,tx=1,ty=1,cx=1,cy=1,success=true)',
        'tap.onFinalize(x=21,y=11,tx=1,ty=1,cx=1,cy=1,success=true)',
      ].join(' | ')
    );
  });

  it('given a Race(pan, tap), when it is dragged past slop, then the pan wins and the tap only finalizes with success false', () => {
    render(
      <GestureProbe
        testID="race-drag"
        build={(record) => {
          const pan = Gesture.Pan();
          const tap = Gesture.Tap();
          attachHandlers(pan, 'pan', record);
          attachHandlers(tap, 'tap', record);
          return Gesture.Race(pan, tap);
        }}
      />
    );
    giveBox('race-drag');
    const target = screen.getByTestId('race-drag');

    fireEvent.pointerDown(target, { clientX: 120, clientY: 60 });
    fireEvent.pointerMove(target, { clientX: 130, clientY: 60 });
    fireEvent.pointerMove(target, { clientX: 150, clientY: 60 });
    fireEvent.pointerUp(target, { clientX: 160, clientY: 60 });

    expect(logOf('race-drag')).toBe(
      [
        'pan.onBegin(x=20,y=10,tx=0,ty=0,cx=0,cy=0)',
        'tap.onBegin(x=20,y=10,tx=0,ty=0,cx=0,cy=0)',
        'pan.onStart(x=30,y=10,tx=10,ty=0,cx=10,cy=0)',
        'pan.onUpdate(x=30,y=10,tx=10,ty=0,cx=10,cy=0)',
        'pan.onChange(x=30,y=10,tx=10,ty=0,cx=10,cy=0)',
        'pan.onUpdate(x=50,y=10,tx=30,ty=0,cx=20,cy=0)',
        'pan.onChange(x=50,y=10,tx=30,ty=0,cx=20,cy=0)',
        'pan.onEnd(x=60,y=10,tx=40,ty=0,cx=10,cy=0,success=true)',
        'pan.onFinalize(x=60,y=10,tx=40,ty=0,cx=10,cy=0,success=true)',
        'tap.onFinalize(x=60,y=10,tx=40,ty=0,cx=10,cy=0,success=false)',
      ].join(' | ')
    );
  });

  it('given a Pan with enabled(false), when it is dragged, then no callback ever fires', () => {
    render(
      <GestureProbe
        testID="disabled"
        build={(record) => {
          const pan = Gesture.Pan().enabled(false);
          attachHandlers(pan, 'pan', record);
          return pan;
        }}
      />
    );
    giveBox('disabled');
    const target = screen.getByTestId('disabled');

    fireEvent.pointerDown(target, { clientX: 120, clientY: 60 });
    fireEvent.pointerMove(target, { clientX: 150, clientY: 60 });
    fireEvent.pointerUp(target, { clientX: 160, clientY: 60 });

    expect(logOf('disabled')).toBe('');
  });
});