import { AvailabilityNotice } from './AvailabilityNotice';
import { T3HostWorkspaces } from './T3HostWorkspaces';
import { desktopSummary } from '../domain/workspaceCatalog';
import { useState } from 'react';
import { ScrollView } from 'react-native';
import { router, useLocalSearchParams } from 'expo-router';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import { Button, ScreenHeader, Text, View } from '@lib/tamagui-design-system';
import type { T3MobilePort } from '../domain/t3Mobile';
import { useCodingAgentPorts } from '../state/ports';
import { ChatKeyboardArea } from './ChatKeyboardArea';
import { T3SessionNavigation } from './T3SessionNavigation';
import { T3ComposerOptions } from './T3ComposerOptions';
import { T3Transcript } from './T3Transcript';
import { ChatScroll } from './ChatScroll';
import { OrcaHome } from './OrcaHome';
import { UnpairedFeed } from './UnpairedFeed';
import { T3PairingScreen } from './T3PairingScreen';
import { useT3Mobile } from '../state/useT3Mobile';

export function T3MobileScreen({ threadId }: { threadId?: string }) {
  const { t3Mobile } = useCodingAgentPorts();
  if (!t3Mobile) throw new Error('Missing T3 mobile adapter');
  return (
    <Session key={threadId ?? 'home'} api={t3Mobile} threadId={threadId} />
  );
}

function Session({ api, threadId }: { api: T3MobilePort; threadId?: string }) {
  const { ChatSurface, Composer, PairedDesktops, HostWorkspaceList } = useAgentComponents();
  const { host, checkout, checkoutId } = useLocalSearchParams<{
    host?: string;
    checkout?: string;
    checkoutId?: string;
  }>();
  const hostOpen = host === '1' && !threadId;
  const state = useT3Mobile(api, hostOpen, threadId, checkoutId);
  const [checkoutMissing, setCheckoutMissing] = useState(false);
  const [pairing, setPairing] = useState(false);
  const goBack = () =>
    router.canGoBack() ? router.back() : router.replace('/coding-agent');
  if (pairing)
    return (
      <T3PairingScreen
        onBack={() => setPairing(false)}
        onPair={async (endpoint, code, signal) => {
          await state.connect(endpoint, code, signal);
          if (!signal.aborted) setPairing(false);
        }}
      />
    );
  if (hostOpen && state.pair !== null && !state.catalog)
    return (
      <HostWorkspaceList
        title="Workspaces"
        sections={[]}
        notice={state.offline ? undefined : state.error || 'Loading workspaces…'}
        availability={state.offline && <AvailabilityNotice reconnecting onAction={state.retry} />}
        retry={!state.offline && state.error ? state.retry : undefined}
        onBack={goBack}
        onSelect={() => {}}
      />
    );
  if (state.pair === null || (!threadId && state.pair === undefined)) {
    return (
      <ChatSurface>
        <OrcaHome
          computer={state.pair}
          feed={state.pair === null && <UnpairedFeed onPairDesktop={() => setPairing(true)} />}
        />
      </ChatSurface>
    );
  }
  if (!threadId && state.pair) {
    if (host === '1' && state.catalog)
      return (
        <T3HostWorkspaces
          key={state.pair.endpoint}
          api={api}
          pair={state.pair}
          catalog={state.catalog}
          offline={state.offline}
          onReconnect={state.retry}
          onUpdated={state.updateCheckout}
          onCatalogUpdated={state.replaceCatalog}
          onBack={goBack}
        />
      );
    return (
      <ChatSurface>
        <OrcaHome
          computer={state.home?.host.name}
          api={api}
          pair={state.pair}
          feed={
            <ScrollView contentContainerStyle={{ paddingHorizontal: 16 }}>
              <PairedDesktops
                desktop={state.offline
                  ? { name: state.home?.host.name ?? new URL(state.pair.endpoint).hostname, summary: 'Offline · Reconnecting…', offline: true }
                  : state.home && { name: state.home.host.name, summary: desktopSummary(state.home) }}
                onOpen={() => router.push('/coding-agent?host=1')}
                onPair={() => setPairing(true)}
              />
              {state.offline ? <AvailabilityNotice reconnecting onAction={state.retry} /> : state.error ? (
                <View>
                  <Text role="alert">{state.error}</Text>
                  <Button onPress={state.retry}>Try again</Button>
                </View>
              ) : (
                !state.home && <Text>Loading workspaces…</Text>
              )}
            </ScrollView>
          }
        />
      </ChatSurface>
    );
  }
  return (
    <ChatSurface>
      {threadId ? (
        <T3SessionNavigation
          key={threadId}
          session={state.imageSession}
          api={api}
          pair={state.pair}
          id={threadId}
          checkout={checkout}
          checkoutId={checkoutId}
          title={
            state.snapshot?.thread.id === threadId
              ? state.snapshot.thread.title
              : 'Thread'
          }
          connected={state.connected}
          onBack={goBack}
          onReconnect={state.retry}
          onMissingChange={setCheckoutMissing}
        />
      ) : (
        <ScreenHeader
          testID="MobileT3Header"
          title="Workspaces"
          onBack={goBack}
        />
      )}
      {state.offline ? <AvailabilityNotice reconnecting onAction={state.retry} /> :
        state.worktreeMissing ? <AvailabilityNotice missing onAction={() => router.replace('/coding-agent?host=1')} /> :
        state.error ? <Text role="alert">{state.error}</Text> : null}
      {state.pair === undefined ? (
        <Text>Opening connection…</Text>
      ) : (
        <ChatKeyboardArea>
          <ChatScroll>
            {state.snapshot && (
              <T3Transcript
                snapshot={state.snapshot}
                session={state.imageSession}
              />
            )}
          </ChatScroll>
          <Composer
            suggestions={state.imageSession}
            leadingControls={
              state.snapshot &&
              state.imageSession && (
                <T3ComposerOptions
                  thread={state.snapshot.thread}
                  session={state.imageSession}
                  newId={api.newId}
                  visible={state.modelPickerOpen}
                  effortPicker={state.effortPicker}
                  onVisibleChange={state.setModelPickerOpen}
                />
              )
            }
            value={state.draft}
            onChangeText={state.changeDraft}
            notice={state.notice}
            onPickSuggestion={state.pickSuggestion}
            onSend={() => { if (!checkoutMissing && !state.worktreeMissing) void state.send(); }}
            sendLabel="Send message"
            canSend={state.connected && !checkoutMissing && !state.worktreeMissing && !!state.draft.trim()}
            sending={state.busy}
            placeholder="Ask anything, @ to mention files, / for commands"
          />
        </ChatKeyboardArea>
      )}
    </ChatSurface>
  );
}
