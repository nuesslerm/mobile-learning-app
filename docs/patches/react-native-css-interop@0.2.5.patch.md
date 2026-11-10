# react-native-css-interop Patch

`printUpgradeWarning` in `dist/runtime/native/render-component.js` stringifies the
upgraded component's `originalProps` with a custom `JSON.stringify` replacer that
recursively walks every enumerable property — including React **elements** in
`children`. In dev, elements carry fiber/context references, and evaluating their
property getters can throw. Concretely, react-navigation's out-of-container guard
objects throw `Couldn't find a navigation context` from a getter, so any cssInterop
upgrade warning fired by a component with element children **crashed the render**
with a completely misleading navigation RedBox (observed: pressing any tab in the
Debug Sheet on iOS — the sheet renders outside the navigation container).

The patch makes the diagnostic printer throw-safe:

- React elements (`value.$$typeof`) are summarized as `"[ReactElement]"` instead of walked;
- each property read is wrapped in try/catch (`"[Throws]"` on getter throw);
- the whole stringify falls back to `"[unserializable props]"`.

The upgrade warning itself still prints — it is real signal (a component upgraded
after initial render will remount); this patch only stops the *printer* from
crashing the app.
