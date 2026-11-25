import { useState } from 'react';
import { ScrollView, TextInput } from 'react-native';
import { router } from 'expo-router';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import {
  Button,
  ListItem,
  ScreenHeader,
  Text,
  TextField,
  View,
} from '@lib/tamagui-design-system';
import type { T3MobilePort } from '../domain/t3Mobile';
import { useCodingAgentPorts } from '../state/ports';
import { ChatKeyboardArea } from './ChatKeyboardArea';
import { T3Transcript } from './T3Transcript';
import { ChatScroll } from './ChatScroll';
import { useT3Mobile } from '../state/useT3Mobile';

export function T3MobileScreen({ threadId }: { threadId?: string }) {
  const { t3Mobile } = useCodingAgentPorts();
  if (!t3Mobile) throw new Error('Missing T3 mobile adapter');
  return <Session api={t3Mobile} threadId={threadId} />;
}

function Session({ api, threadId }: { api: T3MobilePort; threadId?: string }) {
  const state = useT3Mobile(api, threadId);
  const { ChatSurface, Composer } = useAgentComponents();
  const [host, setHost] = useState('');
  const [code, setCode] = useState('');
  return (
    <ChatSurface>
      <ScreenHeader
        testID="MobileT3Header"
        title={
          state.snapshot?.thread.title ?? (threadId ? 'Thread' : 'Workspaces')
        }
        onBack={() =>
          router.canGoBack() ? router.back() : router.replace('/coding-agent')
        }
      />
      {state.error ? (
        <Text accessibilityRole="alert">{state.error}</Text>
      ) : null}
      {state.pair === undefined ? (
        <Text>Opening connection…</Text>
      ) : state.pair === null ? (
        <View p="$4" gap="$3">
          <Text variant="h3">Pair with RCA</Text>
          <TextField
            label="Server address"
            testID="MobilePairEndpoint"
            autoCapitalize="none"
            autoCorrect={false}
            value={host}
            onChangeText={setHost}
          />
          <Text>Pairing code</Text>
          <TextInput
            accessibilityLabel="Pairing code"
            testID="MobilePairCode"
            secureTextEntry
            autoCapitalize="none"
            autoCorrect={false}
            value={code}
            onChangeText={setCode}
          />
          <Button
            testID="MobilePair"
            disabled={state.busy || !host.trim() || !code.trim()}
            onPress={() => void state.connect(host, code)}
          >
            Pair device
          </Button>
        </View>
      ) : threadId ? (
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
      ) : (
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
      )}
    </ChatSurface>
  );
}
