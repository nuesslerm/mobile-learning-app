/**
 * lucide-react-native ships one .mjs per icon but no per-icon .d.ts, so a deep
 * import is untyped without this. Declared here rather than at each import so
 * `ui/icons.ts` stays the only file that knows the on-disk layout.
 */
declare module 'lucide-react-native/dist/esm/icons/*.mjs' {
  import type { LucideIcon } from 'lucide-react-native';

  const icon: LucideIcon;
  export default icon;
}
