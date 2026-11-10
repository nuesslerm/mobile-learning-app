export { renderScreen, FlowProviders, type RenderScreenOptions } from './render';
export { renderRoute, type RenderRouteOptions } from './route';
export { createFlowApp, type FlowApp } from './flowApp';
export { hydrationErrors } from './hydration';
export {
  createSession,
  screenshot,
  type FlowSession,
} from './interactions/session';
export type {
  StrokePoint,
  StrokeTransform,
  StrokeUnits,
} from '@lib/gesture-geometry';
export { setSafeAreaDevice, resetSafeArea, type FlowSafeAreaDevice } from './safeArea';
export { routerHarness, resetRouterHarness, type RouterHarness, type RouterCall } from './router';
export { storageHarness, resetStorageHarness, type StorageHarness } from './storage';
export { createFakeAudioRecorder, type FakeAudioRecorder } from './doubles/fakeAudioRecorder';
export {
  createFakeVoiceEditTransport,
  type FakeVoiceEditTransport,
} from './doubles/fakeVoiceEditTransport';
export { screen, within, waitFor, act, fireEvent, renderHook } from '@testing-library/react';
export { default as userEvent } from '@testing-library/user-event';
export { type FlowCodeEditor } from './interactions/editor';
export { breakAt, logAt, type DebugOptions, type DebugProbe, type Frame, type Stop } from './debugger';
export { expectWarning } from './vitest/warningGate';
