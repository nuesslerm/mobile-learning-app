import type { Blind75Problem } from '../domain';
import { Text } from '@lib/tamagui-design-system';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import type { ReactNode } from 'react';
import { YStack } from '@lib/tamagui-design-system/primitives';

import { MermaidDiagram } from './MermaidDiagram';

type Props = {
  problem: Blind75Problem;
  /**
   * Called when the sheet dismisses itself (pan-down, backdrop tap, explicit
   * close). The route mounting this wires it to `router.back()` -- see
   * `docs/overlays-as-routes.md`.
   */
  onClose: () => void;
};

/**
 * Renders the subset of markdown the catalog actually contains: fenced code
 * blocks, `**bold**` sub-headings and plain lines. A markdown library would be
 * a dependency for three cases that are already enumerated here.
 */
function renderExamples(markdown: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  let key = 0;
  for (const part of markdown.split(/(```[\s\S]*?```)/g)) {
    if (part.startsWith('```')) {
      const code = part.replace(/^```\n?/, '').replace(/\n?```$/, '');
      nodes.push(
        <YStack
          key={key++}
          bg="$color3"
          rounded="$3"
          p="$2.5"
          testID="ProblemDescriptionCode"
        >
          <Text variant="code">{code}</Text>
        </YStack>
      );
      continue;
    }
    for (const line of part.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      if (trimmed.startsWith('**') && trimmed.endsWith('**')) {
        nodes.push(
          <Text key={key++} variant="strong">
            {trimmed.slice(2, -2)}
          </Text>
        );
      } else {
        nodes.push(<Text key={key++}>{trimmed}</Text>);
      }
    }
  }
  return nodes;
}

function renderConstraints(markdown: string): ReactNode[] {
  return markdown
    .split('\n')
    .filter((line) => line.trim())
    .map((line) => {
      const text = line.replace(/^-\s*/, '').replace(/`/g, '');
      return <Text key={text}>{`• ${text}`}</Text>;
    });
}

function Section({
  title,
  children,
  testID,
}: {
  title: string;
  children: ReactNode;
  testID: string;
}) {
  return (
    <YStack gap="$2" mt="$2" testID={testID}>
      <Text variant="h4">{title}</Text>
      {children}
    </YStack>
  );
}

export function ProblemDescriptionSheet({ problem, onClose }: Props) {
  return (
    <Sheet
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      snapPoints={[90, 60]}
      testID="ProblemDescriptionSheet"
    >
      {/* This sheet had no header, and therefore no close control. On a real
          device that left it covering the whole screen with no way out: the
          backdrop tap did nothing and drag-to-dismiss is invisible. */}
      <SheetHeader testID="ProblemDescriptionSheetHeader" onClose={onClose}>
        <Text variant="large" numberOfLines={1}>
          {problem.title}
        </Text>
        <Text variant="muted" numberOfLines={1}>
          {`${problem.difficulty.toUpperCase()} · ${problem.category}`}
        </Text>
      </SheetHeader>

      <Sheet.ScrollView testID="ProblemDescriptionScroll">
        <YStack gap="$3" pb="$10">
          {/* The difficulty/category badge moved into the sheet header, which
              now carries it next to the title. Two copies a scroll apart was
              one too many. */}
          <Text testID="ProblemDescriptionBody">{problem.description}</Text>

          {problem.examples ? (
            <Section title="Examples" testID="ProblemDescriptionExamples">
              {renderExamples(problem.examples)}
            </Section>
          ) : null}

          {problem.constraints ? (
            <Section title="Constraints" testID="ProblemDescriptionConstraints">
              {renderConstraints(problem.constraints)}
            </Section>
          ) : null}

          {problem.diagram ? (
            <Section title="Diagram" testID="ProblemDescriptionDiagram">
              <MermaidDiagram code={problem.diagram} problemId={problem.id} />
            </Section>
          ) : null}

          {problem.hints.length > 0 ? (
            <Section title="Hints" testID="ProblemDescriptionHints">
              {problem.hints.map((hint, i) => (
                <Text key={hint}>{`${i + 1}. ${hint}`}</Text>
              ))}
            </Section>
          ) : null}
        </YStack>
      </Sheet.ScrollView>
    </Sheet>
  );
}
