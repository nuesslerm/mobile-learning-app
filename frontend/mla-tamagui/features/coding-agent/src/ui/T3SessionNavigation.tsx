import { chatRoute } from '../domain/workspaceCatalog';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import { Logger } from '@lib/logging';
import { router } from 'expo-router';
import { useEffect, useRef, useState } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type { Pairing } from '../domain/mobile';
import type { SessionContext, T3Conversation, T3MobilePort } from '../domain/t3Mobile';

export function T3SessionNavigation({
  session,
  api,
  pair,
  id,
  title,
  checkout,
  connected,
  onBack,
  onReconnect,
}: {
  session?: T3Conversation;
  api: T3MobilePort;
  pair?: Pairing | null;
  id: string;
  checkout?: string;
  title: string;
  connected: boolean;
  onBack: () => void;
  onReconnect: () => void;
}) {
  const { SessionHeader, SessionTabsSheet } = useAgentComponents();
  const { top, bottom } = useSafeAreaInsets();
  const [context, setContext] = useState<SessionContext>();
  const [error, setError] = useState('');
  const [open, setOpen] = useState(false);
  const [revision, setRevision] = useState(0);
  const selection = useRef(0);
  useEffect(() => () => { selection.current += 1; }, [session]);
  const dismiss = () => {
    selection.current += 1;
    setOpen(false);
  };
  const refresh = () => setRevision((value) => value + 1);
  useEffect(() => {
    if (!pair || !connected) return;
    const controller = new AbortController();
    api.sessionContext(pair, id, controller.signal).then(
      (result) => {
        if (controller.signal.aborted) return;
        setContext(result);
        setError('');
      },
      () => {
        if (controller.signal.aborted) return;
        Logger.warn('coding_agent.t3.session_context_failed');
        setError('Could not load workspace tabs.');
      }
    );
    return () => controller.abort();
  }, [api, pair, id, connected, revision]);

  const tabs = context?.sessions ?? [];
  const sessions = tabs.some((tab) => tab.id === id)
    ? tabs
    : [...tabs, { id, title }];
  const workspace = checkout ?? context?.workspace ?? 'Terminal';
  async function selectTab(next: string) {
    const request = ++selection.current;
    if (next !== id && tabs.find((tab) => tab.id === next)?.needsProjection) {
      if (!session) {
        setError('Reconnect to open this tab.');
        return;
      }
      try {
        await session.openTab(next);
      } catch (cause) {
        Logger.withError(cause).warn('coding_agent.t3.open_tab_failed');
        if (request === selection.current) setError('Could not open the selected tab.');
        return;
      }
    }
    if (request !== selection.current) return;
    dismiss();
    if (next !== id)
      router.replace(chatRoute(next, checkout));
  }

  return (
    <>
      <SessionHeader
        files={session}
        onOpenTextSettings={() => router.push('/coding-agent/native-chat-settings')}
        workspace={workspace}
        title={sessions.find((s) => s.id === id)?.title ?? title}
        connected={connected}
        topInset={top}
        tabCount={sessions.length}
        onBack={onBack}
        onReconnect={onReconnect}
        onShowTabs={() => {
          setOpen(true);
          refresh();
        }}
      />
      {open && (
        <SessionTabsSheet
          visible
          workspace={workspace}
          activeId={id}
          sessions={sessions}
          bottomInset={bottom}
          error={error || undefined}
          onRetry={refresh}
          onClose={dismiss}
          onSelect={(next) => { void selectTab(next); }}
        />
      )}
    </>
  );
}
