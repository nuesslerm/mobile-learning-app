import { useEffect, useState } from 'react';
import {
  Button,
  Platform,
  ScrollView,
  Text,
  View,
  useColorScheme,
} from 'react-native';
import { router } from 'expo-router';
import {
  SafeAreaView,
  useSafeAreaInsets,
} from 'react-native-safe-area-context';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import { Logger } from '@lib/logging';
import { chatRoute } from '../../domain/workspaceCatalog';
import type { Pairing } from '../../domain/mobile';
import {
  WorktreeMissing,
  RcaUnavailable,
  type T3MobilePort,
} from '../../domain/t3Mobile';
import type {
  TabAgent,
  WorkspaceTabsPort,
} from '../../domain/workspaceTabs/tabs';
import { useWorkspaceTabs } from '../../state/workspaceTabs/useWorkspaceTabs';
import { ChatKeyboardArea } from '../ChatKeyboardArea';
import { AvailabilityNotice } from '../AvailabilityNotice';
import { TerminalTab } from './TerminalTab';
import { BrowserTab } from './BrowserTab';
import { MarkdownNoteTab } from './MarkdownNoteTab';

type Props = {
  api: WorkspaceTabsPort;
  mobile: T3MobilePort;
  pair: Pairing;
  checkout: string;
  name: string;
  showNew: boolean;
  onBack: () => void;
};
export function WorkspaceTabsScreen({
  api,
  mobile,
  pair,
  checkout,
  name,
  showNew,
  onBack,
}: Props) {
  const { layout, error, busy, run, foreground } = useWorkspaceTabs(
    api,
    pair,
    checkout
  );
  const [sheet, setSheet] = useState<'new' | 'tabs'>();
  const [agents, setAgents] = useState<TabAgent[]>([]);
  const [chats, setChats] = useState<{ id: string; title: string }[]>([]);
  const [agentError, setAgentError] = useState('');
  const { top, bottom } = useSafeAreaInsets();
  const color = useColorScheme() === 'dark' ? '#fafafa' : '#0a0a0a';
  const { ChatSurface, SessionHeader, SessionTabsSheet, NewTabSheet } =
    useAgentComponents();
  useEffect(() => {
    if (showNew) setSheet('new');
  }, [showNew]);
  useEffect(() => {
    let live = true;
    if (sheet === 'new')
      api
        .agents(pair)
        .then((rows) => {
          if (live) setAgents(rows);
        })
        .catch((cause) => {
          Logger.withError(cause).warn('coding_agent.workspace_agents_failed');
          if (live) setAgentError('Could not load desktop agents.');
        });
    return () => {
      live = false;
    };
  }, [api, pair, sheet]);
  useEffect(() => {
    const abort = new AbortController();
    mobile
      .checkoutSessions(pair, checkout, abort.signal)
      .then((result) => {
        if (!abort.signal.aborted) setChats(result.sessions);
      })
      .catch((cause) => {
        if (!abort.signal.aborted)
          Logger.withError(cause).warn('coding_agent.workspace_chats_failed');
      });
    return () => abort.abort();
  }, [mobile, pair, checkout, sheet]);
  const active = layout?.tabs.find((t) => t.id === layout.active);
  const [fullscreenTab, setFullscreenTab] = useState<string>();
  const fullscreen = active?.kind === 'browser' && fullscreenTab === active.id;
  useEffect(() => setFullscreenTab(undefined), [active?.id]);
  const tabs = [...chats, ...(layout?.tabs ?? [])];
  const select = async (id: string) => {
    setSheet(undefined);
    if (layout?.tabs.some((t) => t.id === id)) {
      await run({ action: 'select', id });
      return;
    }
    if (await run({ action: 'select', id: null }))
      router.replace(chatRoute(id, name, checkout));
  };
  const startAgent = async (id: string) => {
    setSheet(undefined);
    const agent = agents.find((a) => a.id === id);
    if (!agent || !layout) return;
    try {
      const next = await api.startAgent(pair, layout, agent);
      await run({ action: 'select', id: null });
      router.replace(chatRoute(next, name, checkout));
    } catch (cause) {
      Logger.withError(cause).warn('coding_agent.workspace_start_agent_failed');
      setAgentError('Could not start ' + agent.name + ' in this checkout.');
    }
  };
  const browserZoom = Platform.OS === 'ios' && active?.kind === 'browser';
  const SessionViewport = browserZoom ? ScrollView : View;
  return (
    <ChatSurface>
      <SafeAreaView
        edges={fullscreen ? ['top', 'left', 'right'] : ['left', 'right']}
        style={{ flex: 1 }}
      >
        <SessionViewport
          style={{ flex: 1 }}
          {...(browserZoom
            ? {
                maximumZoomScale: 3.5,
                minimumZoomScale: 1,
                pinchGestureEnabled: true,
                bounces: false,
                keyboardShouldPersistTaps: 'always' as const,
                contentInsetAdjustmentBehavior: 'never' as const,
                showsVerticalScrollIndicator: false,
                showsHorizontalScrollIndicator: false,
                contentContainerStyle: { flex: 1 },
              }
            : {})}
        >
          {!fullscreen && (
            <SessionHeader
              workspace={name}
              title={active?.title ?? name}
              connected={!!layout && !error}
              tabs={tabs}
              activeId={active?.id}
              onSelectTab={(id) => void select(id)}
              branch={tabs.length === 1 ? '1 tab' : tabs.length + ' tabs'}
              onOpenSourceControl={() =>
                router.push(
                  `/coding-agent/source-control?checkoutId=${encodeURIComponent(checkout)}`
                )
              }
              tabCount={tabs.length}
              topInset={top}
              directory={layout?.directory}
              onBack={onBack}
              onReconnect={() => void run()}
              onShowTabs={() => setSheet('tabs')}
              onNewTab={() => setSheet('new')}
            />
          )}
          {error instanceof WorktreeMissing ||
          error instanceof RcaUnavailable ? (
            <AvailabilityNotice
              missing={error instanceof WorktreeMissing}
              reconnecting={error instanceof RcaUnavailable}
              onAction={() =>
                error instanceof WorktreeMissing
                  ? router.replace('/coding-agent?host=1')
                  : void run()
              }
            />
          ) : error || agentError ? (
            <Text accessibilityRole="alert" style={{ color, padding: 12 }}>
              {error?.message ?? agentError}
            </Text>
          ) : null}
          {active &&
          layout &&
          foreground &&
          !(error instanceof WorktreeMissing) ? (
            <ChatKeyboardArea
              alignToScreenBottom={active.kind === 'browser'}
              restingBottom={active.kind === 'markdown' ? 4 : undefined}
            >
              {active.kind === 'terminal' && (
                <TerminalTab
                  key={active.id}
                  api={api}
                  pair={pair}
                  layout={layout}
                  tab={active}
                />
              )}
              {active.kind === 'browser' && (
                <BrowserTab
                  api={api}
                  pair={pair}
                  layout={layout}
                  tab={active}
                  color={color}
                  fullscreen={fullscreen}
                  toggleFullscreen={() =>
                    setFullscreenTab(fullscreen ? undefined : active.id)
                  }
                />
              )}
              {active.kind === 'markdown' && (
                <MarkdownNoteTab
                  key={active.id}
                  api={api}
                  pair={pair}
                  checkout={checkout}
                  directory={layout.directory}
                  path={active.resource}
                  color={color}
                />
              )}
            </ChatKeyboardArea>
          ) : layout && !active && !error ? (
            <View
              style={{
                flex: 1,
                alignItems: 'center',
                justifyContent: 'center',
              }}
            >
              <Button
                title="New Tab"
                disabled={busy}
                onPress={() => setSheet('new')}
              />
            </View>
          ) : null}
          {sheet === 'new' && layout && (
            <NewTabSheet
              workspace={name}
              agents={agents}
              bottomInset={bottom}
              onClose={() => setSheet(undefined)}
              onAgent={(id) => void startAgent(id)}
              onCreate={(kind, url) => {
                setSheet(undefined);
                void run({
                  action: 'create',
                  kind,
                  url,
                  id: mobile.newId().replace(/-/g, '').slice(0, 20),
                });
              }}
            />
          )}
          {sheet === 'tabs' && (
            <SessionTabsSheet
              visible
              workspace={name}
              sessions={tabs}
              activeId={active?.id ?? ''}
              bottomInset={bottom}
              onClose={() => setSheet(undefined)}
              onRetry={() => void run()}
              onSelect={(id) => void select(id)}
              closeableIds={layout?.tabs.map((t) => t.id)}
              onCloseTab={(id) => void run({ action: 'close', id })}
            />
          )}
        </SessionViewport>
      </SafeAreaView>
    </ChatSurface>
  );
}
