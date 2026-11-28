import { useSkin } from '@lib/skin';
import { Logger } from '@lib/logging';
import { Button, ScreenHeader, Text } from '@lib/tamagui-design-system';
import { router, useLocalSearchParams } from 'expo-router';
import { useEffect, useState } from 'react';
import { SafeAreaView } from 'react-native-safe-area-context';
import { RcaUnavailable, WorktreeMissing } from '../domain/t3Mobile';
import type { Pairing } from '../domain/mobile';
import { chatRoute, type CheckoutRow } from '../domain/workspaceCatalog';
import { useCodingAgentPorts } from '../state/ports';
import { T3SourceControl } from './T3SourceControl';

export function ChatSourceControl() {
  const { Spinner } = useSkin();
  const { t3Mobile: api } = useCodingAgentPorts();
  const { thread, checkoutId } = useLocalSearchParams<{
    thread?: string;
    checkoutId?: string;
  }>();
  const [target, setTarget] = useState<{ pair: Pairing; row: CheckoutRow }>();
  const [error, setError] = useState('');
  const [attempt, retry] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setTarget(undefined);
    setError('');
    async function load() {
      if (!api || (!thread && !checkoutId)) {
        setError('Open Source Control from a chat.');
        return;
      }
      const pair = await api.restore();
      if (controller.signal.aborted) return;
      if (!pair) {
        setError('Pair a desktop to open Source Control.');
        return;
      }
      const row = !thread ? (await api.workspaces(pair, controller.signal)).checkouts.find(row => row.id===checkoutId) : await api.threadCheckout(
        pair,
        thread,
        checkoutId,
        controller.signal
      );
      if (!row || row.missing) throw new WorktreeMissing();
      if (!controller.signal.aborted) setTarget({ pair, row });
    }
    load().catch((cause) => {
      if (controller.signal.aborted) return;
      Logger.withError(cause).warn('coding_agent.t3.thread_checkout_failed');
      setError(cause instanceof WorktreeMissing || cause instanceof RcaUnavailable ? cause.message : 'Could not load this chat’s checkout.');
    });
    return () => controller.abort();
  }, [api, thread, checkoutId, attempt]);
  function back() {
    if (router.canGoBack()) router.back();
    else
      router.replace(
        thread
          ? chatRoute(thread, target?.row.name, checkoutId)
          : '/coding-agent'
      );
  }
  if (api && target)
    return (
      <T3SourceControl
        api={api}
        pair={target.pair}
        checkout={target.row}
        backLabel="Back to chat"
        onBack={back}
      />
    );
  return (
    <SafeAreaView style={{ flex: 1 }}>
      <ScreenHeader
        testID="ChatSourceControlHeader"
        title="Source Control"
        onBack={back}
      />
      {error ? (
        <>
          <Text role="alert">{error}</Text>
          <Button onPress={() => retry((n) => n + 1)}>Try again</Button>
        </>
      ) : (
        <Spinner accessibilityLabel="Loading checkout" />
      )}
    </SafeAreaView>
  );
}
