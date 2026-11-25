import { ScreenHeader } from '@lib/tamagui-design-system';

import { LoginScreen } from '../ui/LoginScreen';
import { backOrReplace, useRouteAction } from '../ui/useRouteAction';

export default function LeetCodeLoginRoute() {
  const leave = useRouteAction();
  return <>
    <ScreenHeader placement="navigator" glass testID="LeetCodeLoginHeader" title="Sign in" subtitle="LeetCode" onBack={() => leave(() => backOrReplace('/'))} />
    <LoginScreen />
  </>;
}
