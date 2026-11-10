import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  View,
} from '@lib/tamagui-design-system';
import type { EditAlternative } from '@lib/voice-edit-dsl';
import React from 'react';
import { PanResponder } from 'react-native';

import { PlanTab } from '../plan';
import { ProblemEditorScreen } from './ProblemEditorScreen';

const SWIPE_ACTIVATE_DX = 20;
const SWIPE_THRESHOLD_PX = 60;

type WorkspaceApi = {
  getCode(): string;
  applyEdit(alt: EditAlternative): { modifiedText: string };
  language: string;
};

type WorkspaceTab = 'plan' | 'code';

export function ProblemWorkspace({ id, seed }: { id?: string; seed?: string }) {
  const [tab, setTab] = React.useState<WorkspaceTab>('code');
  const [api, setApi] = React.useState<WorkspaceApi | null>(null);

  const panResponder = React.useMemo(
    () =>
      PanResponder.create({
        onMoveShouldSetPanResponder: (_event, gesture) =>
          Math.abs(gesture.dx) > SWIPE_ACTIVATE_DX &&
          Math.abs(gesture.dx) > Math.abs(gesture.dy),
        onPanResponderRelease: (_event, gesture) => {
          if (gesture.dx > SWIPE_THRESHOLD_PX) setTab('plan');
          else if (gesture.dx < -SWIPE_THRESHOLD_PX) setTab('code');
        },
      }),
    []
  );

  const handleTabChange = React.useCallback(
    (next: string) => setTab(next as WorkspaceTab),
    []
  );

  return (
    <View flex={1} bg="$background">
      {/* The responder sits on the tab shell, so a swipe is picked up over the
          tab bar and the Plan tab. The Code tab's editor is a web view that
          keeps its own gestures, so switching back is a tap on Code. */}
      <Tabs
        flex={1}
        value={tab}
        onValueChange={handleTabChange}
        {...panResponder.panHandlers}
      >
        <TabsList testID="ProblemWorkspaceTabs">
          <TabsTrigger value="plan" testID="ProblemWorkspaceTab-plan">
            Plan
          </TabsTrigger>
          <TabsTrigger value="code" testID="ProblemWorkspaceTab-code">
            Code
          </TabsTrigger>
        </TabsList>
        <TabsContent
          value="code"
          forceMount
          display={tab === 'code' ? 'flex' : 'none'}
        >
          <ProblemEditorScreen id={id} seed={seed} onWorkspaceApi={setApi} />
        </TabsContent>
        <TabsContent
          value="plan"
          forceMount
          display={tab === 'plan' ? 'flex' : 'none'}
        >
          {/* The editor is force-mounted beside this tab, so its API arrives
              on the first frame or two. Until then the plan is fully usable
              and only realising is held back -- never a dead screen. */}
          <PlanTab
            problemId={id ?? ''}
            language={api?.language ?? 'python'}
            getCode={api?.getCode ?? (() => '')}
            applyEdit={api?.applyEdit ?? (() => ({ modifiedText: '' }))}
            canRealize={api !== null}
          />
        </TabsContent>
      </Tabs>
    </View>
  );
}
