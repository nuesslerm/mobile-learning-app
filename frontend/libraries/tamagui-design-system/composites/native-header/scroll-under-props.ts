import { nativeHeaderActive, type HeaderEnv } from './header-env';

export type ScrollUnderProps = {
  contentInsetAdjustmentBehavior?: 'automatic';
};

/** Props that let a list scroll under a transparent native bar; empty where the Tamagui header draws, so the DOM stays as it was. */
export function scrollUnderProps(env: HeaderEnv): ScrollUnderProps {
  return nativeHeaderActive(env)
    ? { contentInsetAdjustmentBehavior: 'automatic' }
    : {};
}
