import {
  Button,
  CodeBlock,
  ComposerBase,
  DiffView,
  Markdown,
  type ParsedDiffFile,
  View,
} from '@lib/tamagui-design-system';
import { useState } from 'react';

import { Compare } from '../Compare';

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

export function CodeBlockItem() {
  return (
    <Compare
      testID="ShowcaseCodeBlock"
      tamagui={
        <CodeBlock
          code={"const greeting = 'hello';\nconsole.log(greeting);\n"}
          language="ts"
          title="snippet.ts"
          testID="Showcase-CodingAgent-CodeBlock"
        />
      }
    />
  );
}

export function MarkdownItem() {
  return (
    <Compare
      testID="ShowcaseMarkdown"
      tamagui={
        <View testID="Showcase-CodingAgent-Markdown">
          <Markdown
            content={
              'Here is a fenced block:\n\n```ts\nconst answer = 42;\nconsole.log(answer);\n```\n'
            }
          />
        </View>
      }
    />
  );
}

export function DiffViewItem() {
  return (
    <Compare
      testID="ShowcaseDiffView"
      tamagui={
        <DiffView
          files={CODING_AGENT_DIFF}
          testID="Showcase-CodingAgent-DiffView"
        />
      }
    />
  );
}

export function ComposerItem() {
  const [text, setText] = useState('');
  const [isRunning, setIsRunning] = useState(false);
  return (
    <Compare
      testID="ShowcaseComposer"
      tamagui={
        <>
          <ComposerBase
            value={text}
            onChangeText={setText}
            onSend={() => setText('')}
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
        </>
      }
    />
  );
}
