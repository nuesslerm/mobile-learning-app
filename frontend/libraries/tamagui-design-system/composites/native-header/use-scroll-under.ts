import { scrollUnderProps, type ScrollUnderProps } from './scroll-under-props';
import { useHeaderEnv } from './use-header-env';

export function useScrollUnder(optedIn: boolean): ScrollUnderProps {
  return scrollUnderProps(useHeaderEnv(optedIn));
}
