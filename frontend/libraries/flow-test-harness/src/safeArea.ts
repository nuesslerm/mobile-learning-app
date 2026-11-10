// The safe-area stub (stubs/react-native-safe-area-context.cjs) reports a phone's insets, iPhone 15 Pro by default.
// A flow test that needs another screen names it; the setup file puts the default back after every test.
export type FlowSafeAreaDevice = 'iphone-15-pro' | 'iphone-se' | 'ipad-pro-11' | 'none';

type Scope = typeof globalThis & { __FLOW_SAFE_AREA__?: FlowSafeAreaDevice };

export function setSafeAreaDevice(device: FlowSafeAreaDevice): void {
  (globalThis as Scope).__FLOW_SAFE_AREA__ = device;
}

export function resetSafeArea(): void {
  delete (globalThis as Scope).__FLOW_SAFE_AREA__;
}
