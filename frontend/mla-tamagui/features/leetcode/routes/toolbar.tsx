import { ScreenHeader } from '@lib/tamagui-design-system';

import { ToolbarConfigScreen } from '../ui/ToolbarConfigScreen';
import { backOrReplace, useRouteAction } from '../ui/useRouteAction';

export default function LeetCodeToolbarConfigRoute() {
  const leave = useRouteAction();
  return <>
    <ScreenHeader placement="navigator" glass testID="ToolbarConfigHeader" title="Toolbar" subtitle="Editor shortcuts" onBack={() => leave(() => backOrReplace('/'))} />
    <ToolbarConfigScreen />
  </>;
}
