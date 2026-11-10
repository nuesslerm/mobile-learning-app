import * as React from 'react';

// `tamagui/setup`, which importing the `tamagui` barrel ran.
(globalThis as Record<string, unknown>).React ||= React;
if (typeof requestAnimationFrame === 'undefined') {
  (globalThis as Record<string, unknown>).requestAnimationFrame =
    typeof setImmediate === 'undefined' ? setTimeout : setImmediate;
}
