'use strict';
// Callable module object (see lucide stub): the default export is the sheet
// component and the named exports render passthrough Views in jsdom.
const React = require('react');
const RN = require('react-native-web');
const Box = (props) =>
  React.createElement(RN.View, { testID: props.testID }, props.children);
// `data-active-offset-y` mirrors the one prop the design-system Sheet sets only on web, so a flow test can assert each platform branch.
const Sheet = (props) =>
  React.createElement(
    RN.View,
    {
      testID: props.testID,
      dataSet: {
        sheet: 'true',
        ...(props.activeOffsetY
          ? { activeOffsetY: JSON.stringify(props.activeOffsetY) }
          : {}),
      },
    },
    props.children
  );
const named = {
  __esModule: true,
  BottomSheet: Sheet,
  BottomSheetModal: Sheet,
  BottomSheetModalProvider: (p) => p.children ?? null,
  BottomSheetView: Box,
  BottomSheetScrollView: (p) =>
    React.createElement(RN.ScrollView, p, p.children),
  BottomSheetBackdrop: Box,
  BottomSheetHandle: Box,
  BottomSheetFooter: Box,
  BottomSheetTextInput: (p) => React.createElement(RN.TextInput, p),
  BottomSheetFlatList: (p) => React.createElement(RN.FlatList, p),
  useBottomSheet: () => ({
    close() {},
    expand() {},
    collapse() {},
    snapToIndex() {},
    snapToPosition() {},
  }),
  useBottomSheetModal: () => ({ dismiss() {}, dismissAll() {} }),
  useBottomSheetDynamicSnapPoints: () => ({
    animatedSnapPoints: { value: ['CONTENT_HEIGHT'] },
    animatedHandleHeight: { value: 0 },
    animatedContentHeight: { value: 0 },
    handleContentLayout() {},
  }),
};
// Own enumerable properties, not a Proxy: the ESM interop enumerates them to
// build the namespace, so named imports resolve.
module.exports = Object.assign(Sheet, named, { default: Sheet });
