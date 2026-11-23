export type HeaderEnv = {
  /** `Platform.OS`, read by the caller: this file imports nothing from react-native. */
  platform: string;
  /** The persisted native-headers setting. */
  nativeHeaders: boolean;
  /** The screen asked for the native bar with its `glass` prop. */
  optedIn: boolean;
};

/** The system header draws on every iOS, glass or not, when the setting is on and the screen opted in; Android and web keep the Tamagui header. */
export function nativeHeaderActive(env: HeaderEnv): boolean {
  return env.platform === 'ios' && env.optedIn && env.nativeHeaders;
}
