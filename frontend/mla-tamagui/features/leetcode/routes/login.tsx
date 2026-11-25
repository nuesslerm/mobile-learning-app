import { ScreenHeader } from '@lib/tamagui-design-system';
import { router } from 'expo-router';

import { LoginScreen } from '../ui/LoginScreen';
import { useRouteAction } from '../ui/useRouteAction';

export default function LeetCodeLoginRoute() {
  const leave = useRouteAction();
  return <>
    <ScreenHeader placement="navigator" glass testID="LeetCodeLoginHeader" title="Sign in" subtitle="LeetCode" onBack={() => leave(() => router.back())} />
    <LoginScreen />
  </>;
}
