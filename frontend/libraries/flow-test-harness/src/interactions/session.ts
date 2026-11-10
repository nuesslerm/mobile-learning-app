// Session API: extends Testing Library's RenderResult with faithful
// interactions -- tap, type, drag, stroke, layout -- addressed by testID and
// driven inside act, plus the real CodeMirror editor once it has hydrated.
import { mountedCodeEditor, type FlowCodeEditor } from './editor';
import {
  strokeSamples,
  toPageCoordinates,
  transformPath,
  type StrokePoint,
  type StrokeTransform,
  type StrokeUnits,
} from '@lib/gesture-geometry';
import { act, fireEvent, type RenderResult } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { layoutBoxes, setLayout, type LayoutBox } from './layout';
import { snapshotPage } from './snapshot';

type StrokeOptions = {
  units?: StrokeUnits;
  curve?: boolean;
  samplesPerSegment?: number;
  transform?: StrokeTransform;
};

export type FlowSession = RenderResult & {
  tap(testID: string): Promise<void>;
  type(testID: string, text: string): Promise<void>;
  layout(testID: string, box: LayoutBox): Promise<void>;
  drag(
    testID: string,
    from: StrokePoint,
    to: StrokePoint,
    opts?: { units?: StrokeUnits }
  ): Promise<void>;
  stroke(
    testID: string,
    points: StrokePoint[],
    opts?: StrokeOptions
  ): Promise<{ samples: number }>;
  /** The real CodeMirror editor in its WebView frame, once hydrated. */
  editor(): Promise<FlowCodeEditor>;
  /** Freezes the screen as `name`; `wf flow run` paints it, and compares it with its golden under --goldens. */
  screenshot(name: string): Promise<string>;
  /** The old name of `screenshot`. */
  snapshot(name: string): Promise<string>;
};

async function tapOn(result: RenderResult, testID: string): Promise<void> {
  const element = result.getByTestId(testID);
  const user = userEvent.setup();
  await user.pointer([
    { keys: '[MouseLeft]', target: element },
    { keys: '[/MouseLeft]', target: element },
  ]);
}

async function typeInto(
  result: RenderResult,
  testID: string,
  text: string
): Promise<void> {
  const element = result.getByTestId(testID);
  const user = userEvent.setup();
  await user.type(element, text);
}

function layOut(result: RenderResult, testID: string, box: LayoutBox): void {
  setLayout(result.getByTestId(testID), box);
}

async function strokeOn(
  result: RenderResult,
  testID: string,
  points: StrokePoint[],
  opts?: StrokeOptions
): Promise<{ samples: number }> {
  const element = result.getByTestId(testID);
  const box = element.getBoundingClientRect();
  const path = toPageCoordinates(
    opts?.transform ? transformPath(points, opts.transform) : points,
    box,
    opts?.units
  );
  const samples = strokeSamples(path, {
    curve: opts?.curve,
    ...(opts?.samplesPerSegment !== undefined
      ? { samplesPerSegment: opts.samplesPerSegment }
      : {}),
  });
  const start = samples[0];
  const last = samples[samples.length - 1];
  act(() => {
    fireEvent.pointerDown(element, { clientX: start.x, clientY: start.y });
    for (const sample of samples.slice(1)) {
      fireEvent.pointerMove(element, { clientX: sample.x, clientY: sample.y });
    }
    fireEvent.pointerUp(element, { clientX: last.x, clientY: last.y });
  });
  return { samples: samples.length };
}

/** The screen as it is now, kept as the screenshot `name` of this test (see FlowSession.screenshot). */
export async function screenshot(name: string): Promise<string> {
  let written = '';
  act(() => {
    written = snapshotPage(name, { layouts: layoutBoxes() });
  });
  return written;
}

export function createSession(result: RenderResult): FlowSession {
  const session = result as FlowSession;
  session.tap = (testID) => tapOn(result, testID);
  session.type = (testID, text) => typeInto(result, testID, text);
  session.layout = (testID, box) =>
    Promise.resolve(layOut(result, testID, box));
  session.stroke = (testID, points, opts) =>
    strokeOn(result, testID, points, opts);
  session.drag = async (testID, from, to, opts) => {
    await session.stroke(testID, [from, to], opts);
  };
  session.editor = () => mountedCodeEditor(result.container);
  session.screenshot = screenshot;
  session.snapshot = screenshot;
  return session;
}
