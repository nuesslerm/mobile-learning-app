import { useAgentComponents } from '@lib/agent-ui-contracts';
import { Logger } from '@lib/logging';
import { router } from 'expo-router';
import { useEffect, useState } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type { Pairing } from '../domain/mobile';
import type { SessionContext, T3MobilePort } from '../domain/t3Mobile';

export function T3SessionNavigation({
  api,
  pair,
  id,
  title,
  connected,
  onBack,
  onReconnect,
}: {
  api: T3MobilePort;
  pair?: Pairing | null;
  id: string;
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
  const workspace = context?.workspace ?? 'Terminal';
  return (
    <>
      <SessionHeader
        workspace={workspace}
        title={title}
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
          onClose={() => setOpen(false)}
          onSelect={(next) => {
            setOpen(false);
            if (next !== id)
              router.replace(
                '/coding-agent/' +
                  encodeURIComponent(next) +
                  '?presentation=orca&transport=t3'
              );
          }}
        />
      )}
    </>
  );
}
