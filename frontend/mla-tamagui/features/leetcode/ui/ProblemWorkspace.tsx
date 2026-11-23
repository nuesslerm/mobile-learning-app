import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  View,
} from '@lib/tamagui-design-system';
import React from 'react';
import { PanResponder, ScrollView } from 'react-native';

import { PlanTab, type PlanEditor } from '../plan';
import { useBlockViewSettingStore } from '../state/blockViewSettingStore';
import { useLeetCodeFlag } from '../state/useLeetCodeFlag';
import { BlockView, type BlocksSource } from './blocks/BlockView';
import { ProblemEditorScreen } from './ProblemEditorScreen';

const SWIPE_ACTIVATE_DX = 20;
const SWIPE_THRESHOLD_PX = 60;

// What the plan tab holds until the editor beside it has handed over its own: realising is held back, so these are never called.
const NO_EDITOR: PlanEditor = {
  language: 'python',
  basis: async () => null,
  applyEdit: async () => ({ ok: false, reason: 'not-ready', code: 'not-ready' }),
};

type WorkspaceTab = 'plan' | 'code' | 'blocks';

export function ProblemWorkspace({ id, seed, lang }: { id?: string; seed?: string; lang?: 'javascript' }) {
  const aiEdit = useLeetCodeFlag('aiEdit');
  const blockView = useBlockViewSettingStore((s) => s.blockView);
  // Off by default, the code stays wired: see domain/featureFlags.ts. With neither tab on, the editor screen is the one view.
  return aiEdit || blockView ? (
    <TabbedWorkspace id={id} seed={seed} lang={lang} plan={aiEdit} blocks={blockView} />
  ) : (
    <View flex={1} bg="$background">
      <ProblemEditorScreen id={id} seed={seed} lang={lang} />
    </View>
  );
}

function TabbedWorkspace({ id, seed, lang, plan, blocks }: { id?: string; seed?: string; lang?: 'javascript'; plan: boolean; blocks: boolean }) {
  const [picked, setTab] = React.useState<WorkspaceTab>('code');
  const [api, setApi] = React.useState<PlanEditor | null>(null);
  const [blocksSource, setBlocksSource] = React.useState<BlocksSource | null>(null);
  const order = React.useMemo<WorkspaceTab[]>(() => [...(plan ? (['plan'] as const) : []), 'code', ...(blocks ? (['blocks'] as const) : [])], [plan, blocks]);
  const tab = order.includes(picked) ? picked : 'code';
  const tabRef = React.useRef(tab);
  const orderRef = React.useRef(order);
  tabRef.current = tab;
  orderRef.current = order;
  const consoleOpenRef = React.useRef(false);
  const handleConsoleOpenChange = React.useCallback((open: boolean) => {
    consoleOpenRef.current = open;
  }, []);

  const panResponder = React.useMemo(
    () =>
      PanResponder.create({
        // the Blocks tab is all block drags: the grip's drag has the touch, and a tab is chosen by a tap there
        onMoveShouldSetPanResponder: (_event, gesture) =>
          !consoleOpenRef.current &&
          tabRef.current !== 'blocks' &&
          Math.abs(gesture.dx) > SWIPE_ACTIVATE_DX &&
          Math.abs(gesture.dx) > Math.abs(gesture.dy),
        onPanResponderRelease: (_event, gesture) => {
          const at = orderRef.current.indexOf(tabRef.current);
          const next = orderRef.current[gesture.dx > SWIPE_THRESHOLD_PX ? at - 1 : gesture.dx < -SWIPE_THRESHOLD_PX ? at + 1 : at];
          if (next) setTab(next);
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
          {plan ? (
            <TabsTrigger value="plan" testID="ProblemWorkspaceTab-plan">
              Plan
            </TabsTrigger>
          ) : null}
          <TabsTrigger value="code" testID="ProblemWorkspaceTab-code">
            Code
          </TabsTrigger>
          {blocks ? (
            <TabsTrigger value="blocks" testID="ProblemWorkspaceTab-blocks">
              Blocks
            </TabsTrigger>
          ) : null}
        </TabsList>
        <TabsContent
          value="code"
          forceMount
          display={tab === 'code' ? 'flex' : 'none'}
        >
          <ProblemEditorScreen id={id} seed={seed} lang={lang} onWorkspaceApi={setApi} active={tab === 'code'} onConsoleOpenChange={handleConsoleOpenChange} onBlocksSource={setBlocksSource} />
        </TabsContent>
        {plan ? (
        <TabsContent
          value="plan"
          forceMount
          display={tab === 'plan' ? 'flex' : 'none'}
        >
          {/* The editor is force-mounted beside this tab, so its API arrives
              on the first frame or two. Until then the plan is fully usable
              and only realising is held back -- never a dead screen. */}
          <PlanTab problemId={id ?? ''} {...(api ?? NO_EDITOR)} canRealize={api !== null} />
        </TabsContent>
        ) : null}
        {blocks ? (
          <TabsContent value="blocks" display={tab === 'blocks' ? 'flex' : 'none'}>
            {tab === 'blocks' && blocksSource ? (
              <ScrollView testID="ProblemWorkspaceBlocks">
                <BlockView {...blocksSource} />
              </ScrollView>
            ) : null}
          </TabsContent>
        ) : null}
      </Tabs>
    </View>
  );
}
