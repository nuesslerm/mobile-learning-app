# react-native-reanimated Patch

`getShadowNodeWrapperFromRef` in `fabricUtils` called `ref.getNativeScrollRef().__internalInstanceHandle`
without null-checking the return value. With `@gorhom/bottom-sheet` scrollables, `getNativeScrollRef()`
returns `null` during the layout-effect ref attach phase, causing a hard crash:
`Cannot read properties of null (reading '__internalInstanceHandle')`.

The patch guards the call: if `getNativeScrollRef()` returns null, fall back to `findHostInstance`
instead of dereferencing. A secondary fix adds optional chaining to `findHostInstance(ref)` since that
can also return null in edge cases.
