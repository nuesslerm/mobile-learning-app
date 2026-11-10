'use strict';
const React = require('react');
const RN = require('react-native-web');

// A phone's insets, not zeros: the browser has no notch, so a screen that forgets the safe area looks right in
// jsdom and sits under the Dynamic Island on a device. The numbers are the device-lab presets'
// (tooling/browser-harness/contracts), and `setSafeAreaDevice` (from @lib/flow-test-harness) switches them.
const DEVICES = {
  'iphone-15-pro': { insets: { top: 59, right: 0, bottom: 34, left: 0 }, frame: { x: 0, y: 0, width: 393, height: 852 } },
  'iphone-se': { insets: { top: 20, right: 0, bottom: 0, left: 0 }, frame: { x: 0, y: 0, width: 375, height: 667 } },
  'ipad-pro-11': { insets: { top: 24, right: 0, bottom: 20, left: 0 }, frame: { x: 0, y: 0, width: 834, height: 1194 } },
  none: { insets: { top: 0, right: 0, bottom: 0, left: 0 }, frame: { x: 0, y: 0, width: 390, height: 844 } },
};
const DEFAULT = 'iphone-15-pro';
const current = () => DEVICES[globalThis.__FLOW_SAFE_AREA__ ?? DEFAULT] ?? DEVICES[DEFAULT];

const seeded = DEVICES[DEFAULT];
module.exports = {
  __esModule: true,
  FLOW_SAFE_AREA_DEVICES: DEVICES,
  FLOW_SAFE_AREA_DEFAULT: DEFAULT,
  SafeAreaProvider: (p) => React.createElement(React.Fragment, null, p.children),
  SafeAreaView: (p) => React.createElement(RN.View, p, p.children),
  SafeAreaInsetsContext: React.createContext(seeded.insets),
  SafeAreaFrameContext: React.createContext(seeded.frame),
  useSafeAreaInsets: () => current().insets,
  useSafeAreaFrame: () => current().frame,
  initialWindowMetrics: { insets: seeded.insets, frame: seeded.frame },
  SafeAreaListener: (p) => p.children ?? null,
};
