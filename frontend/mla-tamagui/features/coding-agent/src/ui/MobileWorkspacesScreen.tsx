import { useState } from 'react';
import { ScrollView } from 'react-native';
import { router } from 'expo-router';
import { Button, ListItem, ScreenHeader, Text, TextField, View } from '@lib/tamagui-design-system';
import type { MobilePort, RepositoryTask } from '../domain/mobile';
import { useMobileHome } from '../state/useMobileHome';
import { useCodingAgentPorts } from '../state/ports';
import { ThreadListScreen } from './ThreadListScreen';
import { DispatchRunsSection } from './DispatchRunsSection';

export function MobileWorkspacesScreen({ api, destination }: { api: MobilePort; destination?: string }) {
  const home = useMobileHome(api);
  const [host, setHost] = useState('');
  const [code, setCode] = useState('');
  const [repo, setRepo] = useState('');
  const [branch, setBranch] = useState('');
  const [base, setBase] = useState('origin/dev');
  const [title, setTitle] = useState('');
  const [creating, setCreating] = useState(false);
  const open = (id: string) => router.push(`/coding-agent/${encodeURIComponent(id)}`);
  const start = (task?: RepositoryTask) => home.perform(async () => {
    if (!home.pair || !home.selected) return;
    const id = await api.createThread(home.pair, home.selected, task?.title ?? title.trim(), 'codex', task?.number);
    setTitle('');
    open(id);
  });
  const row = (id: string, label: string, action: () => void) => (
    <ListItem key={id} testID={id} showChevron onPress={action}><Text>{label}</Text></ListItem>
  );
  return <View flex={1} bg="$background">
    <ScreenHeader glass title={home.selected ?? (home.pair ? 'Workspaces' : 'Coding agent')}
      onBack={home.selected ? () => home.select(undefined) : undefined} testID="MobileAgentHeader" />
    <ScrollView keyboardShouldPersistTaps="handled">
      <View px="$4" py="$3" gap="$3">
        {home.error ? <Text color="$red10" accessibilityRole="alert">{home.error}</Text> : null}
        {!home.ready ? <Text>Opening Coding agent…</Text> : !home.pair ? <>
          <Text variant="h3">Pair with RCA</Text>
          <TextField testID="MobilePairEndpoint" label="Server address" autoCapitalize="none" autoCorrect={false}
            value={host} onChangeText={setHost} placeholder="https://…" />
          <TextField testID="MobilePairCode" label="Pairing code" secureTextEntry autoCapitalize="none"
            value={code} onChangeText={setCode} />
          <Button testID="MobilePair" disabled={!host.trim() || !code.trim()} onPress={() => void home.perform(async () => {
            const saved = await api.pair(host, code);
            setCode('');
            home.setConnection(saved);
            if (destination) router.replace({ pathname: '/coding-agent/[threadId]', params: { threadId: destination, paired: saved.endpoint } });
          })}>Pair device</Button>
        </> : home.selected ? <>
          <Text variant="h3">Threads</Text>
          {home.threads.map(t => row('MobileThread-' + t.id, t.title || t.id, () => open(t.id)))}
          {!home.loading && !home.threads.length ? <Text variant="muted">No threads yet.</Text> : null}
          <TextField testID="MobileThreadTitle" label="Thread title" value={title} onChangeText={setTitle} />
          <Button testID="MobileNewThread" disabled={!title.trim()} onPress={() => void start()}>Start thread</Button>
          <Text variant="h3">Repository tasks</Text>
          {home.tasks.map(t => row('MobileTask-' + t.number, '#' + t.number + ' ' + t.title, () => void start(t)))}
          {!home.loading && !home.tasks.length && !home.error ? <Text variant="muted">No open tasks.</Text> : null}
        </> : <>
          {home.workspaces.map(w => row('MobileWorkspace-' + w.workspace, w.workspace, () => home.select(w.workspace)))}
          <Button testID="MobileCreateWorkspace" onPress={() => setCreating(!creating)}>{creating ? 'Cancel' : 'New workspace'}</Button>
          {creating ? <>
            <Text variant="strong">Repository</Text>
            {home.workspaces.filter(w => w.kind === 'root').map(w => row('MobileRepo-' + w.workspace,
              (repo === w.workspace ? '✓ ' : '') + w.workspace, () => setRepo(w.workspace)))}
            <TextField testID="MobileWorkspaceName" label="Workspace and branch name" value={branch} onChangeText={setBranch} autoCapitalize="none" />
            <TextField testID="MobileWorkspaceBase" label="Start from" value={base} onChangeText={setBase} autoCapitalize="none" />
            <Button testID="MobileSaveWorkspace" disabled={!repo || !branch.trim() || !base.trim()} onPress={() => void home.perform(async () => {
              if (!home.pair) return;
              const id = await api.createWorkspace(home.pair, repo, branch.trim(), base.trim());
              setCreating(false); setBranch(''); home.select(id);
            })}>Create workspace</Button>
          </> : null}
          <DispatchRunsSection />
          <Button testID="MobileRevoke" variant="ghost" onPress={() => void home.disconnect()}>Unpair device</Button>
        </>}
        {home.loading && home.pair ? <Text variant="muted">Loading…</Text> : null}
      </View>
    </ScrollView>
  </View>;
}

export function CodingAgentHome() {
  const { mobile } = useCodingAgentPorts();
  return mobile ? <MobileWorkspacesScreen api={mobile} /> : <ThreadListScreen />;
}
