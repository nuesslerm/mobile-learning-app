import {
  AlertDialog,
  Badge,
  Button,
  Card,
  Chip,
  CodeBlock,
  ComposerBase,
  DiffView,
  Icon,
  InsetGroup,
  ListItem,
  Markdown,
  type ParsedDiffFile,
  SegmentedControl,
  Separator,
  Text,
  TextField,
  View,
} from '@lib/tamagui-design-system';
import {
  Code,
  Rocket,
  Settings,
  Sparkles,
  Trash2,
} from '@lib/tamagui-design-system/icons';
import { useState } from 'react';
import { ScrollView, XStack } from '@lib/tamagui-design-system/primitives';

const CODING_AGENT_DIFF: ParsedDiffFile[] = [
  {
    path: 'src/index.ts',
    additions: 2,
    deletions: 1,
    hunks: [
      {
        header: '@@ -1,3 +1,4 @@',
        lines: [
          { kind: 'ctx', text: 'export function greet() {', oldNo: 1, newNo: 1 },
          { kind: 'del', text: '  return "hi";', oldNo: 2 },
          { kind: 'add', text: '  const name = "world";', newNo: 2 },
          { kind: 'add', text: '  return `hello, ${name}`;', newNo: 3 },
          { kind: 'ctx', text: '}', oldNo: 3, newNo: 4 },
        ],
      },
    ],
  },
  {
    path: 'README.md',
    additions: 1,
    deletions: 0,
    hunks: [
      {
        header: '@@ -10,1 +10,2 @@',
        lines: [
          { kind: 'ctx', text: '## Usage', oldNo: 10, newNo: 10 },
          { kind: 'add', text: 'Run `npm install` before use.', newNo: 11 },
        ],
      },
    ],
  },
];

/**
 * Exercises the design system's real components rather than raw Tamagui, so
 * what renders here is the thing the leetcode port will be built from.
 *
 * testIDs are hand-written and semantic. This app is exempt from the repo's
 * testID lint machinery, but the browser check needs stable selectors.
 */
export default function ShowcaseScreen() {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [name, setName] = useState('');
  const [chipMode, setChipMode] = useState<'build' | 'plan'>('build');
  const [segment, setSegment] = useState<'build' | 'plan'>('build');
  const [composerText, setComposerText] = useState('');
  const [isRunning, setIsRunning] = useState(false);

  return (
    <ScrollView bg="$background">
      <View gap="$4" p="$4" pt="$8">
        <Text variant="h1" testID="ShowcaseHeading">
          Design system
        </Text>
        <Text variant="muted" testID="ShowcaseSubtitle">
          Tamagui components, same names as app #1
        </Text>

        <Separator />

        <XStack gap="$2" flexWrap="wrap">
          <Badge testID="BadgeDefault">default</Badge>
          <Badge variant="primary" testID="BadgePrimary">
            primary
          </Badge>
          <Badge variant="success" testID="BadgeSuccess">
            success
          </Badge>
          <Badge variant="destructive" testID="BadgeDestructive">
            destructive
          </Badge>
          <Badge variant="outline" testID="BadgeOutline">
            outline
          </Badge>
        </XStack>

        <XStack gap="$2" flexWrap="wrap">
          <Button testID="ButtonDefault" icon={Rocket}>
            Default
          </Button>
          <Button variant="secondary" testID="ButtonSecondary">
            Secondary
          </Button>
          <Button variant="outline" testID="ButtonOutline">
            Outline
          </Button>
          <Button variant="ghost" testID="ButtonGhost">
            Ghost
          </Button>
        </XStack>

        <TextField
          testID="ShowcaseNameField"
          label="Your name"
          placeholder="Ada Lovelace"
          value={name}
          onChangeText={setName}
          help="Rendered by the DS TextField"
        />

        {/* The grouped inset list -- one surface, hairline separators, no
            bordered card per row. App #1's design system has no equivalent. */}
        <Text variant="small" testID="GroupLabel">
          GROUPED LIST
        </Text>
        <InsetGroup testID="ShowcaseGroup">
          <ListItem
            testID="RowSettings"
            variant="grouped"
            showChevron
            onPress={() => {}}
          >
            <XStack gap="$2" items="center">
              <Icon as={Settings} size={16} color="$color11" />
              <Text testID="RowSettingsLabel">Settings</Text>
            </XStack>
          </ListItem>
          <ListItem
            testID="RowSecond"
            variant="grouped"
            showChevron
            onPress={() => {}}
          >
            <Text testID="RowSecondLabel">Second row</Text>
          </ListItem>
          <ListItem testID="RowThird" variant="grouped">
            <Text testID="RowThirdLabel">No chevron, not pressable</Text>
          </ListItem>
        </InsetGroup>

        <Separator />

        <Text variant="h2" testID="ShowcaseCodingAgentHeading">
          Coding agent
        </Text>
        <Text variant="muted" testID="ShowcaseCodingAgentSubtitle">
          DS primitives the coding-agent feature is built on.
        </Text>

        <Text variant="small" testID="GroupLabel">
          CARDS
        </Text>
        <Card tone="default" testID="Showcase-CodingAgent-CardDefault">
          <Text>default</Text>
        </Card>
        <Card tone="accent" testID="Showcase-CodingAgent-CardAccent">
          <Text>accent</Text>
        </Card>
        <Card tone="danger" testID="Showcase-CodingAgent-CardDanger">
          <Text>danger</Text>
        </Card>
        <Card tone="warning" testID="Showcase-CodingAgent-CardWarning">
          <Text>warning</Text>
        </Card>

        <Text variant="small" testID="GroupLabel">
          CHIPS
        </Text>
        <XStack gap="$2" flexWrap="wrap">
          <Chip
            label="Build"
            icon={Code}
            selected={chipMode === 'build'}
            onPress={() => setChipMode('build')}
            testID="Showcase-CodingAgent-ChipBuild"
          />
          <Chip
            label="Plan"
            icon={Sparkles}
            selected={chipMode === 'plan'}
            onPress={() => setChipMode('plan')}
            testID="Showcase-CodingAgent-ChipPlan"
          />
          <Chip
            label="Disabled"
            icon={Code}
            selected={false}
            onPress={() => {}}
            testID="Showcase-CodingAgent-ChipDisabled"
          />
        </XStack>

        <Text variant="small" testID="GroupLabel">
          SEGMENTED CONTROL
        </Text>
        <SegmentedControl
          options={[
            { value: 'build', label: 'Build' },
            { value: 'plan', label: 'Plan' },
          ]}
          value={segment}
          onChange={(value) => setSegment(value as 'build' | 'plan')}
          testID="Showcase-CodingAgent-Segmented"
        />

        <Text variant="small" testID="GroupLabel">
          CODE BLOCK
        </Text>
        <CodeBlock
          code={"const greeting = 'hello';\nconsole.log(greeting);\n"}
          language="ts"
          title="snippet.ts"
          testID="Showcase-CodingAgent-CodeBlock"
        />

        <Text variant="small" testID="GroupLabel">
          MARKDOWN
        </Text>
        <View testID="Showcase-CodingAgent-Markdown">
          <Markdown
            content={
              'Here is a fenced block:\n\n```ts\nconst answer = 42;\nconsole.log(answer);\n```\n'
            }
          />
        </View>

        <Text variant="small" testID="GroupLabel">
          DIFF VIEW
        </Text>
        <DiffView
          files={CODING_AGENT_DIFF}
          testID="Showcase-CodingAgent-DiffView"
        />

        <Text variant="small" testID="GroupLabel">
          COMPOSER
        </Text>
        <ComposerBase
          value={composerText}
          onChangeText={setComposerText}
          onSend={() => setComposerText('')}
          onStop={() => setIsRunning(false)}
          isRunning={isRunning}
          placeholder="Compose a coding-agent turn…"
          testID="Showcase-CodingAgent-Composer"
        />
        <Button
          variant="outline"
          testID="Showcase-CodingAgent-ToggleRunning"
          onPress={() => setIsRunning((running) => !running)}
        >
          {isRunning ? 'Stop running' : 'Start running'}
        </Button>

        <Button
          variant="destructive"
          icon={Trash2}
          testID="ShowcaseDestructive"
          onPress={() => setConfirmOpen(true)}
        >
          Open AlertDialog
        </Button>

        <AlertDialog
          open={confirmOpen}
          onOpenChange={setConfirmOpen}
          title="Delete everything?"
          description="This confirms the dialog renders through the DS wrapper."
          confirmLabel="Delete"
          destructive
          onConfirm={() => setConfirmOpen(false)}
          testID="ShowcaseAlert"
        />
      </View>
    </ScrollView>
  );
}
