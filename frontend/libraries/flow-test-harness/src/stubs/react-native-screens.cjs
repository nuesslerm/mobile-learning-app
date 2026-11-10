'use strict';
// react-native-screens ships Flow-typed entry points jsdom cannot parse; on web its overlay is a plain wrapper.
const React = require('react');
const Passthrough = (props) => React.createElement(React.Fragment, null, props.children);
module.exports = {
  __esModule: true,
  FullWindowOverlay: Passthrough,
  Screen: Passthrough,
  ScreenContainer: Passthrough,
  ScreenStack: Passthrough,
  enableScreens: () => {},
  enableFreeze: () => {},
  screensEnabled: () => false,
};
