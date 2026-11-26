import { useCodingAgentPorts } from '../state/ports';
import type { WorkspaceTab } from '../domain/workspaceTabs/tabs';
import { workspaceTabRoute, chatRoute, type CheckoutRow } from '../domain/workspaceCatalog';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import { AvailabilityNotice } from './AvailabilityNotice';
import { Logger } from '@lib/logging';
import { router, useFocusEffect } from 'expo-router';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type { Pairing } from '../domain/mobile';
import { WorktreeMissing, type SessionContext, type T3Conversation, type T3MobilePort } from '../domain/t3Mobile';

export function T3SessionNavigation({
  session,
  api,
  pair,
  id,
  title,
  checkout,
  checkoutId,
  connected,
  onBack,
  onReconnect,
  onMissingChange,
}: {
  session?: T3Conversation;
  api: T3MobilePort;
  pair?: Pairing | null;
  id: string;
  checkout?: string;
  checkoutId?: string;
  title: string;
  connected: boolean;
  onBack: () => void;
  onReconnect: () => void;
  onMissingChange: (missing: boolean) => void;
}) {
  const { SessionHeader, SessionTabsSheet } = useAgentComponents();
  const { top, bottom } = useSafeAreaInsets();
  const [context, setContext] = useState<SessionContext>();
  const [missing, setMissing] = useState(false);
  const [resolved, setResolved] = useState<{ thread: string; checkoutId?: string; pair: Pairing; row: CheckoutRow }>();
  useFocusEffect(useCallback(() => {
    setMissing(false);
    if (!pair || !connected) return;
    const request = new AbortController();
    api.threadCheckout(pair, id, checkoutId, request.signal).then(row => {
      if (!request.signal.aborted) { setResolved({ thread: id, checkoutId, pair, row }); onMissingChange(false); }
    }).catch(cause => {
      if (request.signal.aborted) return;
      setResolved(undefined);
      setMissing(cause instanceof WorktreeMissing);
      if (cause instanceof WorktreeMissing) onMissingChange(true);
      Logger.withError(cause).warn('coding_agent.t3.chat_header_checkout_failed');
    });
    return () => request.abort();
  }, [api, pair, connected, id, checkoutId, onMissingChange]));
  const currentCheckout = resolved && resolved.thread === id && resolved.checkoutId === checkoutId && resolved.pair === pair
    ? resolved.row : undefined;
  const [error, setError] = useState('');
  const [open, setOpen] = useState(false);
  const [revision, setRevision] = useState(0);
  const { workspaceTabs } = useCodingAgentPorts();
  const [resources, setResources] = useState<WorkspaceTab[]>([]);
  const resourceCheckout = currentCheckout?.id;
  useFocusEffect(useCallback(() => {
    let current = true;
    setResources([]);
    if (workspaceTabs && pair && connected && resourceCheckout) {
      workspaceTabs.layout(pair, resourceCheckout).then(layout => {
        if (current) setResources(layout.tabs);
      }).catch(cause => {
        if (current) Logger.withError(cause).warn('coding_agent.chat_resource_tabs_failed');
      });
    }
    return () => { current = false; };
  }, [workspaceTabs, pair, connected, resourceCheckout, revision]));
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
  const workspace = currentCheckout?.name ?? checkout ?? context?.workspace ?? 'Terminal';
  async function selectTab(next: string) {
    const request = ++selection.current;
    if (workspaceTabs && pair && resourceCheckout && resources.some(tab => tab.id === next)) {
      try {
        await workspaceTabs.layout(pair, resourceCheckout, { action: 'select', id: next });
        if (request !== selection.current) return;
        dismiss();
        router.replace(workspaceTabRoute(resourceCheckout, workspace));
      } catch (cause) {
        Logger.withError(cause).warn('coding_agent.open_resource_tab_failed');
        if (request === selection.current) setError('Could not open the selected tab.');
      }
      return;
    }
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
      router.replace(chatRoute(next, checkout, checkoutId));
  }

  return (
    <>
      <SessionHeader
        files={session}
        onOpenTextSettings={() => router.push('/coding-agent/native-chat-settings')}
        workspace={workspace}
        title={sessions.find((s) => s.id === id)?.title ?? title}
        directory={currentCheckout?.directory}
        branch={currentCheckout?.branch}
        connected={connected}
        topInset={top}
        tabCount={sessions.length + resources.length}
        onBack={onBack}
        onReconnect={onReconnect}
        onOpenSourceControl={pair && !missing ? () => router.push('/coding-agent/source-control?thread=' + encodeURIComponent(id) +
          (checkoutId ? '&checkoutId=' + encodeURIComponent(checkoutId) : '')) : undefined}
        onNewTab={currentCheckout && !missing ? () => router.push(workspaceTabRoute(currentCheckout.id, workspace, true)) : undefined}
        onShowTabs={() => {
          setOpen(true);
          refresh();
        }}
      />
      {missing && connected && <AvailabilityNotice missing onAction={() => router.replace('/coding-agent?host=1')} />}
      {open && (
        <SessionTabsSheet
          visible
          workspace={workspace}
          activeId={id}
          sessions={[...sessions, ...resources]}
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
