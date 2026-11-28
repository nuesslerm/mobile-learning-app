/**
 * lucide-react-native ships one .mjs per icon but no per-icon .d.ts, so a deep
 * import is untyped without this. The same declaration lives in the app's
 * tamagui design system; each package declares it for itself, so neither
 * depends on the other's types.
 */
declare module 'lucide-react-native/dist/esm/icons/*.mjs' {
  import type { LucideIcon } from 'lucide-react-native';

  const icon: LucideIcon;
  export default icon;
}
