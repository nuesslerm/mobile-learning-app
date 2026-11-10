'use strict';
const React = require('react');
const RN = require('react-native-web');
const passthrough = new Proxy({}, { get: (_t, name) => RN[name] ?? ((props) => React.createElement('div', props)) });
const noop = () => {};
const val = (v) => ({ value: v });
const AnimatedNS = { View: RN.View, Text: RN.Text, ScrollView: RN.ScrollView, Image: RN.Image, FlatList: RN.FlatList, createAnimatedComponent: (c) => c };
// Babel interop may hand `import Animated` the whole module, so the namespace members sit at top level too.
module.exports = {
  __esModule: true,
  ...AnimatedNS,
  default: AnimatedNS,
  Animated: passthrough,
  useSharedValue: val, useAnimatedStyle: (fn) => (typeof fn === 'function' ? fn() : {}), useDerivedValue: (fn) => val(fn()),
  useAnimatedScrollHandler: () => noop, useAnimatedReaction: noop, useAnimatedRef: () => ({ current: null }),
  withTiming: (v) => v, withSpring: (v) => v, withDelay: (_d, v) => v, withSequence: (...a) => a[a.length - 1], withRepeat: (v) => v,
  runOnJS: (fn) => fn, runOnUI: (fn) => fn, interpolate: (v) => v, Extrapolation: { CLAMP: 'clamp' }, Easing: new Proxy({}, { get: () => (x) => x }),
  createAnimatedComponent: (c) => c, cancelAnimation: noop, useAnimatedProps: (fn) => (typeof fn === 'function' ? fn() : {}),
  FadeIn: { duration() { return this; } }, FadeOut: { duration() { return this; } }, Layout: {}, LinearTransition: {},
};
