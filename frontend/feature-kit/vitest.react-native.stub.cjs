// Stand-in for `react-native` in the feature unit tests: its real index.js is Flow syntax vite-node
// cannot parse. Aliased by vitest.feature.ts and seeded into require.cache by vitest.setup.ts.
'use strict';
const noop = () => undefined;
const subscription = () => ({ remove: noop });

module.exports = {
  Platform: { OS: 'ios', select: (obj) => obj.ios ?? obj.default },
  StyleSheet: { create: (s) => s, flatten: (s) => s, hairlineWidth: 0.5 },
  View: 'View',
  ScrollView: 'ScrollView',
  Text: 'Text',
  TextInput: 'TextInput',
  Pressable: 'Pressable',
  TouchableOpacity: 'TouchableOpacity',
  Image: 'Image',
  ActivityIndicator: 'ActivityIndicator',
  FlatList: 'FlatList',
  SectionList: 'SectionList',
  VirtualizedList: 'VirtualizedList',
  Modal: 'Modal',
  RefreshControl: 'RefreshControl',
  Switch: 'Switch',
  KeyboardAvoidingView: 'KeyboardAvoidingView',
  SafeAreaView: 'SafeAreaView',
  StatusBar: 'StatusBar',
  Touchable: { Mixin: {} },
  PanResponder: {
    create: () => ({
      panHandlers: {
        onMoveShouldSetPanResponder: noop,
        onPanResponderGrant: noop,
        onPanResponderMove: noop,
        onPanResponderEnd: noop,
        onPanResponderRelease: noop,
        onPanResponderTerminate: noop,
        onShouldBlockNativeResponder: noop,
      },
    }),
  },
  Animated: {
    View: 'Animated.View',
    Text: 'Animated.Text',
    ScrollView: 'Animated.ScrollView',
    Image: 'Animated.Image',
    createAnimatedComponent: (c) => c,
  },
  Easing: { inOut: () => () => 0, ease: () => () => 0, linear: () => () => 0 },
  Dimensions: {
    get: () => ({ width: 320, height: 640, scale: 2, fontScale: 1 }),
  },
  PixelRatio: {
    get: () => 2,
    getFontScale: () => 1,
    getPixelSizeForLayoutSize: (n) => n * 2,
  },
  NativeModules: {},
  requireNativeComponent: (n) => n,
  findNodeHandle: () => null,
  unstable_batchedUpdates: (fn) => {
    fn();
  },
  LogBox: { ignoreLogs: noop },
  Appearance: {
    getColorScheme: () => 'light',
    addChangeListener: subscription,
  },
  AppState: { currentState: 'active', addEventListener: subscription },
  Keyboard: { addListener: subscription, dismiss: noop },
  Linking: { openURL: () => Promise.resolve(), addEventListener: subscription },
  Share: { share: () => Promise.resolve() },
  Alert: { alert: noop },
  ToastAndroid: { show: noop, SHORT: 0, LONG: 1, BOTTOM: 0, CENTER: 1, TOP: 2 },
  I18nManager: { isRTL: false, allowRTL: noop, forceRTL: noop },
  AccessibilityInfo: {
    isReduceMotionEnabled: () => Promise.resolve(false),
    isScreenReaderEnabled: () => Promise.resolve(false),
    addEventListener: subscription,
  },
  processColor: (color) => color,
};
