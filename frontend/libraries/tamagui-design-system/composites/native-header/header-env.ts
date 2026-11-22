export type HeaderEnv = {
  /** `Platform.OS`, read by the caller: this file imports nothing from react-native. */
  platform: string;
  /** Both isLiquidGlassAvailable() and isGlassEffectAPIAvailable() report true. */
  liquidGlass: boolean;
  /** The persisted glass-headers setting. */
  glassHeaders: boolean;
  /** The screen asked for the native bar with its `glass` prop. */
  optedIn: boolean;
};

/** The native glass bar draws only on iOS with Liquid Glass, when the setting is on and the screen opted in; everywhere else the Tamagui header stays. */
export function nativeHeaderActive(env: HeaderEnv): boolean {
  return (
    env.platform === 'ios' && env.liquidGlass && env.glassHeaders && env.optedIn
  );
}
