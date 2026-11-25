import { useAgentComponents } from '@lib/agent-ui-contracts';
import { useMemo, useState } from 'react';
import { Keyboard, KeyboardAvoidingView, Platform, ScrollView } from 'react-native';
import { router } from 'expo-router';
import { Button, ComposerBase, ListItem, Markdown, ScreenHeader, Text, TextField, View } from '@lib/tamagui-design-system';
import type { MobilePort } from '../domain/mobile';
import { conversation, type Pending } from '../domain/mobileConversation';
import { useMobileThread } from '../state/useMobileThread';
import { useCodingAgentPorts } from '../state/ports';
import { ThreadScreen } from './ThreadScreen';
import { ChatKeyboardArea } from './ChatKeyboardArea';
import { ChatScroll } from './ChatScroll';
import { MobileWorkspacesScreen } from './MobileWorkspacesScreen';

function RequestCard({ request, respond }: { request: Pending; respond: (value: string) => void }) {
  const [answer, setAnswer] = useState('');
  return <View bg="$color2" p="$3" gap="$2" rounded="$4">
    <Text variant="strong">{request.kind === 'approvals' ? 'Approval requested' : 'Question'}</Text>
    <Text>{request.description}</Text>
    {request.kind === 'approvals' ? <View flexDirection="row" gap="$2">
      <Button onPress={() => respond('accept')}>Allow once</Button>
      <Button variant="outline" onPress={() => respond('decline')}>Decline</Button>
    </View> : <>
      <TextField testID={'Answer-' + request.id} label="Your answer" value={answer} onChangeText={setAnswer} />
      <Button disabled={!answer.trim()} onPress={() => respond(answer.trim())}>Answer</Button>
    </>}
  </View>;
}

export function MobileThreadScreen({ api, id, orca = false }: { api: MobilePort; id: string; orca?: boolean }) {
  const { Composer, MessageBubble } = useAgentComponents();
  const state = useMobileThread(api, id);
  const { rows, pending, children } = useMemo(() => conversation(state.events), [state.events]);
  if (state.legacy) return <ThreadScreen threadId={id} />;
  if (state.pair === null) return <MobileWorkspacesScreen api={api} destination={id} orca={orca} />;
  const TranscriptScroll = orca ? ChatScroll : ScrollView;
  const KeyboardArea = orca ? ChatKeyboardArea : KeyboardAvoidingView;
  return <View flex={1} bg="$background">
    <ScreenHeader glass testID="MobileThreadHeader" title={state.context?.thread.title ?? 'Thread'}
      subtitle={state.context?.thread.workspace} onBack={() => router.canGoBack() ? router.back() : router.replace('/coding-agent')} />
    <KeyboardArea style={{ flex: 1 }} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
    <TranscriptScroll keyboardShouldPersistTaps="handled">
      <View p="$4" gap="$3">
        {state.status ? <Text variant="muted">{state.status}</Text> : null}
        {state.error ? <Text color="$red10" accessibilityRole="alert">{state.error}</Text> : null}
        {state.context?.task ? <Text variant="strong">Task #{state.context.task.number}</Text> : null}
        {rows.map(row => orca && (row.label === 'You' || row.label === 'Agent') ? (
          <MessageBubble key={row.seq} role={row.label === 'You' ? 'user' : 'assistant'}>
            <Markdown content={row.body} />
          </MessageBubble>
        ) : <View key={row.seq} gap="$1">
          <Text variant="strong">{row.label}</Text><Markdown content={row.body} />
        </View>)}
        {pending.map(request => <RequestCard key={request.id} request={request}
          respond={value => void state.respond(request.id, request.kind, value)} />)}
        {children.length ? <Text variant="strong">Subagents</Text> : null}
        {children.map(child => <View key={child.id} gap="$1" testID={'MobileAgent-' + child.id}>
          <ListItem testID={'ChildTranscript-' + child.id} showChevron onPress={() => state.setChildId(state.childId === child.id ? undefined : child.id)}>
            <Text>{child.label}</Text>
          </ListItem>
          <Text variant="muted">{child.status}</Text>
          {state.childId === child.id ? <>
            {state.transcript?.rows.map(item => <View key={item.seq} gap="$1">
              <Text variant="strong">{item.role === 'user' ? 'User' : 'Agent'}</Text>
              <Markdown content={item.text ?? ''} />
            </View>)}
            {state.transcript?.message || !state.transcript
              ? <Text variant="muted">{state.transcript?.message ?? 'Loading transcript…'}</Text> : null}
          </> : null}
        </View>)}
        {state.context?.family.threads.filter(t => t.lineage?.parent === id).map(({ thread }) => (
          <ListItem key={thread.id} testID={'Child-' + thread.id} showChevron onPress={() =>
            router.push({ pathname: '/coding-agent/[threadId]', params: { threadId: thread.id } })}>
            <Text>{thread.title}</Text>
          </ListItem>
        ))}
      </View>
    </TranscriptScroll>
    {orca ? <Composer value={state.draft} onChangeText={state.setDraft}
      placeholder="Ask anything, @ to mention files, / for commands" sendLabel="Send message"
      canSend={!!state.context && !!state.draft.trim()} sending={state.sending}
      onSend={() => { void state.send().then(sent => { if (sent) Keyboard.dismiss(); }); }} /> :
    <ComposerBase testID="MobileComposer" placeholder="Message…" value={state.draft}
      onChangeText={state.setDraft} disabled={!state.context} onSend={() => void state.send()} />}
    </KeyboardArea>
  </View>;
}

export function CodingAgentThread({ id, orca }: { id?: string; orca?: boolean }) {
  const { mobile } = useCodingAgentPorts();
  return mobile && id ? <MobileThreadScreen api={mobile} id={id} orca={orca} /> : <ThreadScreen threadId={id} />;
}
