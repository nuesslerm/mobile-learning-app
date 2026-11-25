import { useState } from 'react';
import { ScrollView } from 'react-native';
import { router } from 'expo-router';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import {
  Button,
  ListItem,
  ScreenHeader,
  Text,
  View,
} from '@lib/tamagui-design-system';
import type { T3MobilePort } from '../domain/t3Mobile';
import { useCodingAgentPorts } from '../state/ports';
import { ChatKeyboardArea } from './ChatKeyboardArea';
import { T3SessionNavigation } from './T3SessionNavigation';
import { T3Transcript } from './T3Transcript';
import { ChatScroll } from './ChatScroll';
import { OrcaHome } from './OrcaHome';
import { UnpairedFeed } from './UnpairedFeed';
import { T3PairingScreen } from './T3PairingScreen';
import { useT3Mobile } from '../state/useT3Mobile';

export function T3MobileScreen({ threadId }: { threadId?: string }) {
  const { t3Mobile } = useCodingAgentPorts();
  if (!t3Mobile) throw new Error('Missing T3 mobile adapter');
  return <Session key={threadId ?? 'home'} api={t3Mobile} threadId={threadId} />;
}

function Session({ api, threadId }: { api: T3MobilePort; threadId?: string }) {
  const state = useT3Mobile(api, threadId);
  const { ChatSurface, Composer, HomeTabHeader } = useAgentComponents();
  const [pairing, setPairing] = useState(false);
  const goBack = () =>
    router.canGoBack()
      ? router.back()
      : router.replace('/coding-agent?presentation=orca&transport=t3');
  if (pairing) return <T3PairingScreen onBack={() => setPairing(false)} onPair={async (endpoint, code, signal) => {
    await state.connect(endpoint, code, signal);
    if (!signal.aborted) setPairing(false);
  }} />;
  if (state.pair === null) {
    return (
      <ChatSurface>
        <OrcaHome
          feed={<UnpairedFeed onPairDesktop={() => setPairing(true)} />}
        />
      </ChatSurface>
    );
  }
  if (!threadId && state.pair)
    return (
      <ChatSurface>
        <OrcaHome
          feed={
            <>
              <HomeTabHeader title="Workspaces" actions={[{id: 'pair', icon: 'add', accessibilityLabel: 'Pair desktop', onPress: () => setPairing(true)}]} />
              {state.error ? (
                <Text accessibilityRole="alert">{state.error}</Text>
              ) : null}
              <ScrollView>
                <View p="$4" gap="$2">
                  {!state.shell ? <Text>Loading workspaces…</Text> : null}
                  {state.shell?.projects.length === 0 ? (
                    <Text>No projects yet.</Text>
                  ) : null}
                  {state.shell?.projects.map((project) => (
                    <View key={project.id}>
                      <Text variant="strong">{project.title}</Text>
                      {state.shell?.threads
                        .filter((thread) => thread.projectId === project.id)
                        .map((thread) => (
                          <ListItem
                            key={thread.id}
                            testID={'MobileThread-' + thread.id}
                            showChevron
                            onPress={() =>
                              router.push(
                                '/coding-agent/' +
                                  encodeURIComponent(thread.id) +
                                  '?presentation=orca&transport=t3'
                              )
                            }
                          >
                            <Text>{thread.title}</Text>
                          </ListItem>
                        ))}
                    </View>
                  ))}
                  <Button onPress={state.retry}>Refresh</Button>
                </View>
              </ScrollView>
            </>
          }
        />
      </ChatSurface>
    );
  return (
    <ChatSurface>
      {threadId ? <T3SessionNavigation key={threadId} api={api} pair={state.pair} id={threadId}
        title={state.snapshot?.thread.id === threadId ? state.snapshot.thread.title : 'Thread'}
        connected={state.connected} onBack={goBack} onReconnect={state.retry} /> :
        <ScreenHeader testID="MobileT3Header" title="Workspaces" onBack={goBack} />}
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
