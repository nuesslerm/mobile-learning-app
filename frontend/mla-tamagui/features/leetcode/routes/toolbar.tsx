import { ScreenHeader } from '@lib/tamagui-design-system';
import { router } from 'expo-router';

import { ToolbarConfigScreen } from '../ui/ToolbarConfigScreen';
import { useRouteAction } from '../ui/useRouteAction';

export default function LeetCodeToolbarConfigRoute() {
  const leave = useRouteAction();
  return <>
    <ScreenHeader placement="navigator" glass testID="ToolbarConfigHeader" title="Toolbar" subtitle="Editor shortcuts" onBack={() => leave(() => router.back())} />
    <ToolbarConfigScreen />
  </>;
}
