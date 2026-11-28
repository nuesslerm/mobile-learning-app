import { useState, type ReactNode } from 'react';
import { ScrollView, View } from 'react-native';

import type { Skin } from '../ports/Skin';
import type { SourceCommit, SourceFile } from '../ports/SourceControl';
import { useSkin } from './SkinContext';
import { useSkinTheme } from './SkinThemeContext';

const noop = () => undefined;
const resolved = async () => undefined;

/** Where the gallery shows each entry of the contract. A new entry does not compile until it is placed here. */
export const GALLERY_ENTRIES = {
  HomeSettingsScreen: 'settings',
  HomeSettingsGroup: 'settings',
  HomeSettingsRow: 'settings',
  HomeSettingsNote: 'settings',
  ChatSizeControl: 'settings',
  ChatSpacingControl: 'settings',
  HomeSurface: 'home',
  HomeTabHeader: 'home',
  HomeSearchField: 'search',
  HomeAgentSection: 'home',
  HomeEmptyState: 'home',
  HomeTabBar: 'home',
  BrandMark: 'home',
  PairedDesktops: 'home',
  UnpairedHome: 'unpaired',
  PairDesktopButton: 'onboarding',
  OnboardingChoice: 'onboarding',
  NotificationChoices: 'onboarding',
  PairingScreen: 'pairing',
  ChatSurface: 'chat',
  SessionHeader: 'chat',
  MessageBubble: 'chat',
  ChatMessageText: 'chat',
  ChatMessageMeta: 'chat',
  ChatToolActivity: 'chat',
  ChatImage: 'chat',
  Composer: 'chat',
  ComposerSendButton: 'chat',
  ComposerAddButton: 'chat',
  ComposerSuggestionList: 'suggestions',
  HostWorkspaceList: 'workspaces',
  SourceControl: 'source-control',
  WorkspaceTabControl: 'tab-controls',
  HomeActionSheet: 'sheet-home-actions',
  NewTabSheet: 'sheet-new-tab',
  SessionTabsSheet: 'sheet-session-tabs',
  PairingCodeSheet: 'sheet-pairing-code',
  ComposerOptions: 'sheet-composer-options',
  WorkspaceActions: 'sheet-workspace-actions',
  WorkspaceOptions: 'sheet-workspace-options',
  BottomSheet: 'sheet-plain',
  ChatImageViewer: 'image-viewer',
  Row: 'layout',
  Stack: 'layout',
  Text: 'layout',
  Button: 'controls',
  TextField: 'controls',
  Switch: 'controls',
  Spinner: 'controls',
  Notice: 'controls',
  HomeSettingsSectionTitle: 'chat-settings',
  HomeSettingsSpacer: 'chat-settings',
  ChatTextPreview: 'chat-settings',
  OptionSheet: 'sheet-options',
} as const satisfies Record<keyof Skin, string>;

export type GallerySection = (typeof GALLERY_ENTRIES)[keyof Skin];

export const GALLERY_SECTIONS = [...new Set(Object.values(GALLERY_ENTRIES))] as readonly GallerySection[];

const FILES: SourceFile[] = [
  { path: 'src/feed/ThreadFeed.tsx', status: 'modified', additions: 18, deletions: 4, binary: false },
  { path: 'src/feed/assets/empty.png', status: 'added', binary: true },
  { path: 'README.md', status: 'deleted', additions: 0, deletions: 12, binary: false },
];

const COMMITS: SourceCommit[] = [
  { sha: 'a1b2c3d4e5f60718', shortSha: 'a1b2c3d', subject: 'Keep the feed pinned to its last row', author: 'Ada', authoredAtMs: 1795700000000, parent: 'f0e1d2c3b4a59687' },
  { sha: 'f0e1d2c3b4a59687', shortSha: 'f0e1d2c', subject: 'Show the pending approval above the composer', author: 'Grace', authoredAtMs: 1795600000000, parent: null },
];

const TABS = [
  { id: 'chat-1', title: 'Fix the flaky resume test', kind: 'chat' },
  { id: 'term-1', title: 'Terminal', kind: 'terminal' },
];

const WORKSPACE_FILES = {
  listDirectory: async (path: string) => ({
    entries: path
      ? [{ path: `${path}/index.ts`, kind: 'file' as const }]
      : [
          { path: 'README.md', kind: 'file' as const },
          { path: 'src', kind: 'directory' as const },
          { path: 'docs', kind: 'directory' as const },
          { path: 'package.json', kind: 'file' as const },
        ],
    truncated: false,
  }),
  readFile: async () => ({ contents: 'export const answer = 42;\n', truncated: false }),
};

const ANSWER = 'The test waits on `setTimeout(0)` while the stream is still flushing.\n\n```ts\nawait vi.waitFor(() =>\n  expect(rows).toHaveLength(3))\n```';

function Caption({ children }: { children: string }) {
  const { HomeSettingsNote } = useSkin();
  return <HomeSettingsNote>{children}</HomeSettingsNote>;
}

function Stage({ children, padded = true }: { children: ReactNode; padded?: boolean }) {
  return <View style={{ gap: 12, padding: padded ? 16 : 0 }}>{children}</View>;
}

function Settings({ topInset }: { topInset: number }) {
  const skin = useSkin();
  const [on, setOn] = useState(true);
  return (
    <skin.HomeSettingsScreen title="Settings" topInset={topInset} onBack={noop}>
      <skin.HomeSettingsNote>A note explains the group beneath it.</skin.HomeSettingsNote>
      <skin.HomeSettingsGroup separatorInset={52}>
        <skin.HomeSettingsRow icon="appearance" label="Opens a screen" navigates onPress={noop} />
        <skin.HomeSettingsRow icon="terminal" label="Shows a value" value="Menlo" navigates onPress={noop} />
        <skin.HomeSettingsRow icon="notifications" label="Carries a switch" toggle={{ value: on, onValueChange: setOn }} />
        <skin.HomeSettingsRow icon="about" label="Offers an action" accessory="Update" onPress={noop} />
      </skin.HomeSettingsGroup>
      <View style={{ height: 16 }} />
      <skin.HomeSettingsGroup separatorInset={16}>
        <skin.HomeSettingsRow label="Picked option" selected onPress={noop} />
        <skin.HomeSettingsRow label="Other option" onPress={noop} />
        <skin.HomeSettingsRow label="Disabled row" disabled onPress={noop} />
      </skin.HomeSettingsGroup>
      <View style={{ height: 16 }} />
      <skin.HomeSettingsGroup separatorInset={16}>
        <skin.ChatSizeControl label="Text size" />
        <skin.ChatSpacingControl label />
      </skin.HomeSettingsGroup>
    </skin.HomeSettingsScreen>
  );
}

function ChatSettings({ topInset }: { topInset: number }) {
  const skin = useSkin();
  return (
    <skin.HomeSettingsScreen title="Chat UI" topInset={topInset} onBack={noop}>
      <skin.ChatTextPreview question="Why is the resume test flaky?" activity="Read 3 files · Ran pnpm test" answer={ANSWER} copyCode={async () => true} />
      <skin.HomeSettingsSectionTitle>TEXT</skin.HomeSettingsSectionTitle>
      <skin.HomeSettingsGroup separatorInset={16}>
        <skin.HomeSettingsRow label="Font" value="System" navigates onPress={noop} />
      </skin.HomeSettingsGroup>
      <skin.HomeSettingsNote>Pinch in a chat to change it; it saves here.</skin.HomeSettingsNote>
      <skin.HomeSettingsSpacer size="lg" />
      <skin.HomeSettingsGroup>
        <skin.HomeSettingsRow label="Reset text settings" disabled onPress={noop} />
      </skin.HomeSettingsGroup>
    </skin.HomeSettingsScreen>
  );
}

function Home({ bottomInset }: { bottomInset: number }) {
  const skin = useSkin();
  return (
    <skin.HomeSurface>
      <skin.HomeTabHeader
        title="Agents"
        actions={[
          { id: 'search', accessibilityLabel: 'Search', icon: 'search', onPress: noop },
          { id: 'options', accessibilityLabel: 'Options', icon: 'options', onPress: noop, active: true },
        ]}
      />
      <ScrollView contentContainerStyle={{ gap: 12, paddingBottom: 16 }}>
        <skin.PairedDesktops desktop={{ name: 'Studio Mac', summary: '3 workspaces · 2 agents working' }} onPair={noop} onOpen={noop} />
        <skin.HomeAgentSection
          title="Working"
          rows={[
            { id: 'a', title: 'Fix the flaky resume test', place: 'mobile-app · main', age: '2m', unread: true, status: 'working' },
            { id: 'b', title: 'Wants to run pnpm test', place: 'mobile-app · feed', age: '8m', unread: false, status: 'waiting' },
            { id: 'c', title: 'Typecheck failed', place: 'backend · api', age: '1h', unread: false, status: 'failed' },
          ]}
          onSelect={noop}
        />
        <View style={{ alignItems: 'center', paddingVertical: 8 }}>
          <skin.BrandMark size={40} />
        </View>
        <skin.HomeEmptyState icon="automations" title="No automations yet" message="Automations you create on the desktop appear here." />
      </ScrollView>
      <skin.HomeTabBar
        tabs={[
          { id: 'feed', label: 'Feed' },
          { id: 'bots', label: 'Bots' },
          { id: 'agents', label: 'Agents', badge: 2 },
          { id: 'usage', label: 'Usage' },
          { id: 'automations', label: 'Automations' },
        ]}
        active="agents"
        bottomInset={bottomInset}
        onSelect={noop}
      />
    </skin.HomeSurface>
  );
}

// The field takes the focus as it does in the app, so this section shows with the keyboard up.
function Search() {
  const skin = useSkin();
  const [query, setQuery] = useState('resume');
  return (
    <skin.HomeSurface>
      <skin.HomeTabHeader title="Agents" actions={[{ id: 'search', accessibilityLabel: 'Search', icon: 'search', onPress: noop, active: true }]} />
      <skin.HomeSearchField value={query} onChangeText={setQuery} placeholder="Search agents" onClear={() => setQuery('')} />
    </skin.HomeSurface>
  );
}

function Unpaired({ bottomInset }: { bottomInset: number }) {
  const { HomeSurface, UnpairedHome } = useSkin();
  return (
    <HomeSurface>
      <UnpairedHome
        contentMaxWidth={480}
        isWideLayout={false}
        bottomInset={bottomInset}
        title="Connect your desktop"
        description="Pair with your computer to check on your agents and drive work from your phone."
        steps={[
          { id: 'open', title: 'Open the desktop app', description: 'Go to Settings, then Mobile, and generate a pairing code.' },
          { id: 'scan', title: 'Scan the code', description: 'Tap Pair desktop below and point your camera at the code.' },
          { id: 'done', title: "You're connected", description: 'Your desktop appears here.' },
        ]}
        pairLabel="Pair desktop"
        onPairDesktop={noop}
      />
    </HomeSurface>
  );
}

function Onboarding() {
  const skin = useSkin();
  return (
    <skin.HomeSurface>
      <Stage>
        <Caption>PairDesktopButton</Caption>
        <skin.PairDesktopButton label="Pair desktop" onPress={noop} />
        <Caption>OnboardingChoice: primary, plain, busy, disabled</Caption>
        <skin.OnboardingChoice label="Continue" primary busy={false} disabled={false} onPress={noop} />
        <skin.OnboardingChoice label="Not now" busy={false} disabled={false} onPress={noop} />
        <skin.OnboardingChoice label="Working" primary busy disabled={false} onPress={noop} />
        <skin.OnboardingChoice label="Unavailable" busy={false} disabled onPress={noop} />
        <Caption>NotificationChoices</Caption>
        <skin.NotificationChoices
          enableLabel="Enable notifications"
          enableAccessibilityLabel="Enable agent notifications"
          skipLabel="Not now"
          skipAccessibilityLabel="Skip notifications for now"
          busyChoice={null}
          disabled={false}
          onChoice={noop}
        />
      </Stage>
    </skin.HomeSurface>
  );
}

function Pairing({ topInset, bottomInset }: Insets) {
  const { PairingScreen } = useSkin();
  return (
    <PairingScreen
      permission="prompt"
      status="scanning"
      topInset={topInset}
      bottomInset={bottomInset}
      keyboardInset={0}
      pasteVisible={false}
      onBack={noop}
      onCamera={noop}
      onPaste={noop}
      onCancel={noop}
      onSubmit={noop}
      onRetry={noop}
    />
  );
}

function Chat({ topInset }: Insets) {
  const skin = useSkin();
  const [draft, setDraft] = useState('Looks right. Ship it');
  return (
    <skin.ChatSurface>
      <skin.SessionHeader
        tabs={TABS}
        activeId="chat-1"
        onSelectTab={noop}
        branch="fix/resume-test"
        directory="/Users/ada/mobile-app"
        onOpenSourceControl={noop}
        onOpenTextSettings={noop}
        files={WORKSPACE_FILES}
        workspace="mobile-app"
        title="Fix the flaky resume test"
        connected
        tabCount={2}
        topInset={topInset}
        onBack={noop}
        onReconnect={noop}
        onShowTabs={noop}
        onNewTab={noop}
      />
      <ScrollView contentContainerStyle={{ paddingVertical: 8 }}>
        <skin.MessageBubble role="user" footer={<skin.ChatMessageMeta time="09:41" accessibleTime="9:41 AM" onCopy={async () => true} />}>
          <skin.ChatMessageText tone="user" text="Why is the resume test flaky?" />
        </skin.MessageBubble>
        <skin.ChatToolActivity
          summary="Read 3 files · Ran pnpm test"
          working={false}
          rows={[
            { id: 't1', name: 'Read', input: 'src/feed/ThreadFeed.tsx', output: '170 lines', status: 'completed' },
            { id: 't2', name: 'Bash', input: 'pnpm test resume', output: '1 failed', status: 'failed' },
          ]}
        />
        <skin.MessageBubble role="assistant">
          <skin.ChatMessageText text={ANSWER} copyCode={async () => true} />
        </skin.MessageBubble>
        <View style={{ paddingHorizontal: 16 }}>
          <skin.ChatImage label="screenshot.png" unavailable onOpen={noop} />
        </View>
      </ScrollView>
      <skin.Composer
        value={draft}
        placeholder="Message the agent"
        sendLabel="Send"
        canSend={draft.length > 0}
        leadingControls={<skin.ComposerAddButton onSelect={noop} onDismiss={noop} />}
        trailingControls={<skin.ComposerSendButton label="Send" canSend={draft.length > 0} onPress={noop} />}
        onChangeText={setDraft}
        onSend={noop}
      />
    </skin.ChatSurface>
  );
}

function Suggestions() {
  const { ChatSurface, ComposerSuggestionList } = useSkin();
  const rows = [
    { kind: 'command' as const, token: '/review', description: 'Review the current diff' },
    { kind: 'skill' as const, token: '/simplify', description: 'Simplify recent changes', hint: 'skill' },
    { kind: 'file' as const, token: 'src/feed/ThreadFeed.tsx' },
  ];
  return (
    <ChatSurface>
      <Stage>
        <ComposerSuggestionList
          source={{
            commands: rows.filter((row) => row.kind === 'command'),
            files: async () => rows.filter((row) => row.kind === 'file'),
            catalog: async () => rows.filter((row) => row.kind === 'skill'),
          }}
          sigil="/"
          query=""
          kind={null}
          onPick={noop}
        />
      </Stage>
    </ChatSurface>
  );
}

function Workspaces() {
  const { HostWorkspaceList } = useSkin();
  const [search, setSearch] = useState('');
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  return (
    <HostWorkspaceList
      title="Studio Mac"
      platform="macOS"
      sections={[
        { key: 'pinned', label: 'Pinned', icon: 'pin', rows: [{ id: 'w1', name: 'fix/resume-test', detail: 'Working · 2m', project: 'mobile-app', unread: true }] },
        {
          key: 'mobile-app',
          label: 'mobile-app',
          icon: 'folder',
          rows: [
            { id: 'w2', name: 'main', detail: 'Idle · 1h' },
            { id: 'w3', name: 'feed/pinned-rows', detail: 'Waiting for approval' },
          ],
        },
      ]}
      disclosure={{
        keys: collapsed,
        ready: true,
        change: (key, isCollapsed) => setCollapsed((keys) => new Set(isCollapsed ? [...keys, key] : [...keys].filter((k) => k !== key))),
      }}
      search={{ value: search, onChange: setSearch }}
      onBack={noop}
      onActions={noop}
      onSelect={noop}
    />
  );
}

function SourceControlSection() {
  const { SourceControl } = useSkin();
  const [tab, setTab] = useState<'changes' | 'pr'>('changes');
  return (
    <SourceControl
      tab={tab}
      selectTab={setTab}
      prPanel={{ result: { branch: 'fix/resume-test', pullRequest: null }, refresh: noop, open: noop, parseReference: () => null, link: resolved }}
      page="changes"
      comparison={{ base: 'main', ahead: 2, files: FILES, limited: false }}
      openFile={noop}
      openBranches={noop}
      switchingBranch={false}
      openHistory={noop}
      openCommit={noop}
      branch="fix/resume-test"
      head="a1b2c3d"
      commits={COMMITS}
      loading={false}
      loadingMore={false}
      onBack={noop}
      onRefresh={noop}
    />
  );
}

function Layout() {
  const { HomeSurface, Row, Stack, Text } = useSkin();
  return (
    <HomeSurface>
      <Stack padding="lg" gap="lg">
        <Stack gap="xs">
          <Text variant="title">Title</Text>
          <Text variant="heading">Heading</Text>
          <Text variant="large">Large</Text>
          <Text variant="strong">Strong</Text>
          <Text>Body text in the default tone.</Text>
          <Text variant="muted">Muted text for a secondary line.</Text>
          <Text variant="small">SMALL LABEL</Text>
        </Stack>
        <Row gap="sm" align="center">
          {(['base', 'group', 'raised'] as const).map((surface) => (
            <Stack key={surface} grow padding="md" radius="md" surface={surface} bordered>
              <Text variant="small">{surface}</Text>
            </Stack>
          ))}
        </Row>
        <Stack padding="lg" gap="sm" radius="lg" surface="accent">
          <Row gap="sm">
            <Stack paddingX="sm" paddingY="xs" radius="pill" surface="onAccent">
              <Text variant="small" tone="onAccent">ON ACCENT</Text>
            </Stack>
          </Row>
          <Text variant="large" tone="onAccent">An accent surface</Text>
          <Text tone="onAccent" dimmed>With a dimmed second line.</Text>
        </Stack>
        <Row justify="between" align="center" padding="md" radius="md" surface="group" bordered>
          <Stack grow>
            <Text variant="muted">A row</Text>
            <Text variant="strong" numberOfLines={1}>with its ends pushed apart</Text>
          </Stack>
          <Text variant="small">END</Text>
        </Row>
      </Stack>
    </HomeSurface>
  );
}

function Controls() {
  const skin = useSkin();
  const [on, setOn] = useState(true);
  const [url, setUrl] = useState('http://localhost:3000');
  return (
    <skin.HomeSurface>
      <skin.Stack padding="lg" gap="lg">
        <skin.Row gap="md" align="center">
          <skin.Button label="Retry" onPress={noop} />
          <skin.Button label="Discard" tone="danger" onPress={noop} />
          <skin.Button label="Disabled" disabled onPress={noop} />
        </skin.Row>
        <skin.Row gap="md" align="center">
          <skin.Switch value={on} onValueChange={setOn} accessibilityLabel="Example switch" />
          <skin.Switch value={false} onValueChange={noop} accessibilityLabel="Disabled switch" disabled />
          <skin.Spinner accessibilityLabel="Loading" />
        </skin.Row>
        <skin.TextField value={url} onChangeText={setUrl} placeholder="Address" keyboard="url" />
        <skin.TextField value="" onChangeText={noop} placeholder="An empty field shows its placeholder" />
      </skin.Stack>
      <skin.Notice title="Worktree unavailable" detail="This worktree may have been deleted or moved on the desktop." actionLabel="View workspaces" onAction={noop} />
    </skin.HomeSurface>
  );
}

function TabControls() {
  const { WorkspaceTabControl } = useSkin();
  const { tabs } = useSkinTheme();
  return (
    <View style={{ flex: 1, backgroundColor: tabs.bg }}>
      <Stage>
        <Caption>Terminal keys</Caption>
        <View style={{ flexDirection: 'row', gap: 8 }}>
          <WorkspaceTabControl label="Esc" terminal onPress={noop} />
          <WorkspaceTabControl label="Ctrl" terminal selected onPress={noop} />
          <WorkspaceTabControl label="Tab" terminal disabled onPress={noop} />
        </View>
        <Caption>Browser tools and keys</Caption>
        <View style={{ flexDirection: 'row', gap: 8 }}>
          <WorkspaceTabControl label="Back" display="‹" browser="tool" onPress={noop} />
          <WorkspaceTabControl label="Forward" display="›" browser="tool" disabled onPress={noop} />
          <WorkspaceTabControl label="Enter" browser="key" onPress={noop} />
        </View>
        <Caption>Plain</Caption>
        <View style={{ flexDirection: 'row', gap: 8 }}>
          <WorkspaceTabControl label="Copy" onPress={noop} />
          <WorkspaceTabControl label="Copy path" selected onPress={noop} />
        </View>
      </Stage>
    </View>
  );
}

type Insets = { topInset: number; bottomInset: number };

function Overlay({ section, topInset, bottomInset }: Insets & { section: GallerySection }) {
  const skin = useSkin();
  const { colors } = useSkinTheme();
  switch (section) {
    case 'sheet-home-actions':
      return (
        <skin.HomeActionSheet
          visible
          groups={[
            [
              { id: 'all', label: 'All agents', checked: true, onPress: noop },
              { id: 'unread', label: 'Unread only', onPress: noop },
            ],
            [{ id: 'archive', label: 'Archive finished', disabled: true, onPress: noop }],
          ]}
          onClose={noop}
        />
      );
    case 'sheet-new-tab':
      return (
        <skin.NewTabSheet
          workspace="mobile-app"
          bottomInset={bottomInset}
          agents={[
            { id: 'claude', name: 'Claude' },
            { id: 'codex', name: 'Codex' },
            { id: 'opencode', name: 'OpenCode' },
          ]}
          onClose={noop}
          onAgent={noop}
          onCreate={noop}
        />
      );
    case 'sheet-session-tabs':
      return (
        <skin.SessionTabsSheet
          visible
          workspace="mobile-app"
          sessions={TABS}
          activeId="chat-1"
          closeableIds={['term-1']}
          onCloseTab={noop}
          bottomInset={bottomInset}
          onClose={noop}
          onRetry={noop}
          onSelect={noop}
        />
      );
    case 'sheet-pairing-code':
      return <skin.PairingCodeSheet visible bottomInset={bottomInset} keyboardInset={0} onCancel={noop} onSubmit={noop} />;
    case 'sheet-composer-options':
      return (
        <skin.ComposerOptions
          visible
          onVisibleChange={noop}
          provider="Claude"
          selectedModel="sonnet"
          permission="ask"
          permissions={[
            { slug: 'ask', name: 'Ask before edits' },
            { slug: 'auto', name: 'Edit automatically' },
          ]}
          effort={{ choices: [{ slug: 'low', name: 'Low' }, { slug: 'high', name: 'High' }], current: 'high', select: resolved }}
          loadModels={async () => [
            { slug: 'sonnet', name: 'Sonnet' },
            { slug: 'opus', name: 'Opus' },
          ]}
          setModel={resolved}
          setPermission={resolved}
        />
      );
    case 'sheet-workspace-actions':
      return (
        <skin.WorkspaceActions
          name="fix/resume-test"
          automaticName="resume-test"
          branch="fix/resume-test"
          pinned
          unread={false}
          saving={false}
          onClose={noop}
          onRename={noop}
          onPin={noop}
          onSourceControl={noop}
          onToggleRead={noop}
          statusPicker={{ choices: [{ id: 'working', label: 'Working' }, { id: 'review', label: 'In review' }], selected: 'working', failed: false }}
          onOpenStatuses={noop}
          onStatus={noop}
        />
      );
    case 'sheet-workspace-options':
      return (
        <skin.WorkspaceOptions
          visible
          groups={[
            { label: 'Group by', segments: true, rows: [{ label: 'Project', selected: true, kind: 'radio', onPress: noop }, { label: 'Status', kind: 'radio', onPress: noop }] },
            { rows: [{ label: 'Show archived', kind: 'switch', selected: false, onPress: noop }, { label: 'Sort', value: 'Recent', kind: 'page', onPress: noop }] },
          ]}
          pageGroups={[]}
          loading={false}
          saving={false}
          resetEnabled
          onOpen={noop}
          onClose={noop}
          onBack={noop}
          onReset={noop}
          onRetry={noop}
        />
      );
    case 'sheet-options':
      return (
        <skin.OptionSheet
          title="Chat font"
          dismissLabel="Dismiss Chat font"
          bottomInset={bottomInset}
          options={[
            { id: 'system', label: 'System', subtitle: 'SF Pro', selected: true },
            { id: 'serif', label: 'Serif', subtitle: 'New York' },
            { id: 'mono', label: 'Monospace', subtitle: 'Same as code' },
          ]}
          onSelect={noop}
          onClose={noop}
        />
      );
    case 'sheet-plain':
      return (
        <skin.BottomSheet label="Dismiss sheet" onClose={noop} style={{ backgroundColor: colors.sheetSurface, borderTopLeftRadius: 22, borderTopRightRadius: 22, padding: 16, paddingBottom: bottomInset + 16 }}>
          {() => (
            <>
              <Caption>A plain bottom sheet holds whatever a screen puts in it.</Caption>
              <skin.OnboardingChoice label="Done" primary busy={false} disabled={false} onPress={noop} />
            </>
          )}
        </skin.BottomSheet>
      );
    case 'image-viewer':
      return (
        <skin.ChatImageViewer
          label="pixel.png"
          uri="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
          topInset={topInset}
          rightInset={0}
          bottomInset={bottomInset}
          onClose={noop}
        />
      );
    default:
      return null;
  }
}

function Index({ onOpen, topInset }: { onOpen: (section: GallerySection) => void; topInset: number }) {
  const skin = useSkin();
  return (
    <skin.HomeSettingsScreen title="Skin gallery" topInset={topInset} onBack={noop}>
      <skin.HomeSettingsNote>Every entry of the skin contract, drawn by the selected skin.</skin.HomeSettingsNote>
      <skin.HomeSettingsGroup separatorInset={16}>
        {GALLERY_SECTIONS.map((section) => (
          <skin.HomeSettingsRow key={section} label={section} navigates onPress={() => onOpen(section)} />
        ))}
      </skin.HomeSettingsGroup>
    </skin.HomeSettingsScreen>
  );
}

/** One section of the gallery with fixed props, or the list of sections when none is named. */
export function SkinGallery({ section, topInset, bottomInset, onOpen }: Insets & { section?: string; onOpen: (section: GallerySection) => void }) {
  const { colors } = useSkinTheme();
  const insets = { topInset, bottomInset };
  const known = GALLERY_SECTIONS.find((candidate) => candidate === section);
  let body: ReactNode;
  switch (known) {
    case undefined:
      body = <Index onOpen={onOpen} topInset={topInset} />;
      break;
    case 'settings':
      body = <Settings topInset={topInset} />;
      break;
    case 'chat-settings':
      body = <ChatSettings topInset={topInset} />;
      break;
    case 'home':
      body = <Home bottomInset={bottomInset} />;
      break;
    case 'search':
      body = <Search />;
      break;
    case 'unpaired':
      body = <Unpaired bottomInset={bottomInset} />;
      break;
    case 'onboarding':
      body = <Onboarding />;
      break;
    case 'pairing':
      return <View testID="SkinGallery-pairing" style={{ flex: 1 }}><Pairing {...insets} /></View>;
    case 'chat':
      body = <Chat {...insets} />;
      break;
    case 'suggestions':
      body = <Suggestions />;
      break;
    case 'workspaces':
      body = <Workspaces />;
      break;
    case 'source-control':
      body = <SourceControlSection />;
      break;
    case 'controls':
      body = <Controls />;
      break;
    case 'layout':
      body = <Layout />;
      break;
    case 'tab-controls':
      body = <TabControls />;
      break;
    default:
      body = <Overlay section={known} {...insets} />;
  }
  const ownsTopInset = known === undefined || known === 'chat' || known === 'settings' || known === 'chat-settings';
  return (
    <View testID={`SkinGallery-${known ?? 'index'}`} style={{ flex: 1, paddingTop: ownsTopInset ? 0 : topInset, backgroundColor: colors.bgBase }}>
      {body}
    </View>
  );
}
