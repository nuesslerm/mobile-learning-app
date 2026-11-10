// `__DEV__` is a Metro/React Native global. The app's own program picks up
// its ambient declaration transitively via `react-native`'s type entry, but
// this package typechecks standalone and has no reason to depend on the full
// `react-native` package just for that global.
declare const __DEV__: boolean;
