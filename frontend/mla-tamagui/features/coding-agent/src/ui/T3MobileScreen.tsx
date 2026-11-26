import { T3HostWorkspaces } from './T3HostWorkspaces';
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
  const state = useT3Mobile(api, threadId);
  const {
    ChatSurface,
    Composer,
    HomeTabHeader,
    HomeSettingsRow,
    HomeSettingsGroup,
  } = useAgentComponents();
  const { host, checkout } = useLocalSearchParams<{
    host?: string;
    checkout?: string;
  }>();
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
  if (state.pair === null) {
    return (
      <ChatSurface>
        <OrcaHome
          feed={<UnpairedFeed onPairDesktop={() => setPairing(true)} />}
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
          onUpdated={state.updateCheckout}
          onBack={goBack}
        />
      );
    return (
      <ChatSurface>
        <OrcaHome
          feed={
            <>
              <HomeTabHeader title="Feed" />
              <ScrollView contentContainerStyle={{ padding: 16 }}>
                <Text variant="strong">Desktops</Text>
                <HomeSettingsGroup>
                  {state.catalog && (
                    <HomeSettingsRow
                      label={state.catalog.host.name}
                      value={state.catalog.checkouts.length + ' workspaces'}
                      navigates
                      accessibilityLabel={'Open ' + state.catalog.host.name}
                      onPress={() => router.push('/coding-agent?host=1')}
                    />
                  )}
                  <HomeSettingsRow
                    icon="add"
                    label="Pair a desktop"
                    onPress={() => setPairing(true)}
                  />
                </HomeSettingsGroup>
                {state.error ? (
                  <View>
                    <Text accessibilityRole="alert">{state.error}</Text>
                    <Button onPress={state.retry}>Try again</Button>
                  </View>
                ) : (
                  !state.catalog && <Text>Loading workspaces…</Text>
                )}
              </ScrollView>
            </>
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
          title={
            state.snapshot?.thread.id === threadId
              ? state.snapshot.thread.title
              : 'Thread'
          }
          connected={state.connected}
          onBack={goBack}
          onReconnect={state.retry}
        />
      ) : (
        <ScreenHeader
          testID="MobileT3Header"
          title="Workspaces"
          onBack={goBack}
        />
      )}
      {state.error ? (
        <Text accessibilityRole="alert">{state.error}</Text>
      ) : null}
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
                />
              )
            }
            value={state.draft}
            onChangeText={state.setDraft}
            onSend={() => void state.send()}
            sendLabel="Send message"
            canSend={state.connected && !!state.draft.trim()}
            sending={state.busy}
            placeholder="Ask anything, @ to mention files, / for commands"
          />
        </ChatKeyboardArea>
      )}
    </ChatSurface>
  );
}
