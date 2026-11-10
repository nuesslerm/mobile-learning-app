/**
 * Deliberately far smaller than mobile-learning-app's 86-line config.
 *
 * That file needs mutually-exclusive overrides to keep NativeWind's
 * `cssInterop` away from `@gorhom/bottom-sheet` and
 * `react-native-gesture-handler` -- cssInterop-wrapped views break
 * `NativeViewGestureHandler`, and there is a related Reanimated `measure()`
 * crash. Tamagui does not wrap components at the JSX-runtime level, so that
 * entire class of conflict does not exist here.
 */
module.exports = function babelConfig(api) {
  api.cache(true);
  return {
    // @lib/logging/babel-preset injects {file, line, fn} into every Logger.*
    // call at compile time, which is what makes a log line traceable back to
    // its source rather than just a message.
    presets: [
      [
        'babel-preset-expo',
        {
          // zustand/middleware reads `import.meta.env` to detect Redux
          // DevTools. In a classic (non-module) web bundle that is a
          // SyntaxError at PARSE time, so zustand's own `try/catch` around it
          // never runs -- the whole chunk fails to evaluate and every store in
          // it is dead. What that looks like from outside is an app that
          // renders its first screen and then ignores every tap, with one
          // console error nobody is reading. This rewrites it to
          // `globalThis.__ExpoImportMetaRegistry`.
          unstable_transformImportMeta: true,
        },
      ],
      '@lib/logging/babel-preset',
    ],
    plugins: [
      [
        '@tamagui/babel-plugin',
        {
          components: ['tamagui'],
          config: './tamagui.config.ts',
          logTimings: true,
          // Extraction OFF, and this is a correctness fix rather than a perf
          // preference. With it on, every element carrying a THEME TOKEN
          // (`bg="$color2"`) vanished from the static export -- the whole
          // screen, since one failed element takes its Suspense boundary with
          // it -- while the same element with a literal (`bg="red"`) rendered
          // fine. Nothing was logged: `expo export` still exited 0 and wrote a
          // page whose body held only the drawer.
          //
          // Diagnosing it needs one warning: Metro's transform cache survives a
          // babel.config.js edit, so toggling this and re-exporting reports the
          // OLD result. `rm -rf node_modules/.cache /tmp/metro-*` first, or the
          // experiment lies to you -- it cost a wrong conclusion here already.
          disableExtraction: true,
        },
      ],
    ],
  };
};
