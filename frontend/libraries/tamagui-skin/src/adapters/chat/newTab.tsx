import type { NewTabSheetProps } from '@lib/skin';
import { Button, Icon, InsetGroup, ListItem, Text, TextField } from '@lib/tamagui-design-system';
import { Asterisk, FileText, Globe, Hexagon, SquareTerminal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { useState } from 'react';

import { ScreenSheet } from '../sheet/ScreenSheet';

const ACTIONS = [
  { kind: 'terminal', title: 'Terminal', icon: SquareTerminal },
  { kind: 'browser', title: 'Browser', icon: Globe },
  { kind: 'markdown', title: 'Markdown Note', icon: FileText },
] as const;

const AGENT_ICONS = { claudeAgent: Asterisk, codex: Hexagon } as const;

/** What a new tab can be: an agent, a terminal, a browser (which asks for its address first) or a note. */
export function NewTabSheet({ workspace, bottomInset, agents, onAgent, onCreate, onClose }: NewTabSheetProps) {
  const [browser, setBrowser] = useState(false);
  const [url, setUrl] = useState('');
  // the sheet leaves before the tab opens, as a person sees it
  const then = (act: () => void) => () => {
    onClose();
    act();
  };
  const openBrowser = then(() => onCreate('browser', url.trim() || undefined));
  return (
    <ScreenSheet onClose={onClose} snapPoints={[60]} handleLabel="Close" testID="skin-new-tab">
      {browser ? (
        <YStack gap="$3" px="$4" pb={Math.max(16, bottomInset)}>
          <YStack gap="$1">
            <Text variant="large" role="heading">
              New Browser
            </Text>
            <Text variant="muted">Enter a URL, or leave blank for a new tab.</Text>
          </YStack>
          <TextField
            testID="skin-new-tab-url"
            aria-label="Browser URL"
            autoFocus
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="url"
            returnKeyType="done"
            selectTextOnFocus
            placeholder="https://example.com"
            placeholderTextColor="$color10"
            value={url}
            onChangeText={setUrl}
            onSubmitEditing={openBrowser}
          />
          <XStack gap="$3" justify="flex-end">
            <Button variant="ghost" glass={false} pressLog="off" onPress={onClose} testID="skin-new-tab-cancel">
              Cancel
            </Button>
            <Button pressLog="off" onPress={openBrowser} testID="skin-new-tab-open">
              Open
            </Button>
          </XStack>
        </YStack>
      ) : (
        <YStack gap="$3" pb={Math.max(16, bottomInset)}>
          <YStack px="$4" gap="$1">
            <Text variant="large" role="heading">
              New Tab
            </Text>
            <Text variant="muted" numberOfLines={1}>
              {workspace}
            </Text>
          </YStack>
          {agents.length > 0 ? (
            <YStack gap="$2">
              <Text variant="muted" textTransform="uppercase" letterSpacing={0.5} mx="$4">
                Agents
              </Text>
              <InsetGroup testID="skin-new-tab-agents">
                {agents.map((agent) => (
                  <ListItem key={agent.id} variant="grouped" pressLog="off" accessibilityLabel={agent.name} onPress={then(() => onAgent(agent.id))} testID="skin-new-tab-agent">
                    <XStack items="center" gap="$3">
                      <Icon as={AGENT_ICONS[agent.id as keyof typeof AGENT_ICONS] ?? SquareTerminal} size={18} color="$color11" />
                      <Text>{agent.name}</Text>
                    </XStack>
                  </ListItem>
                ))}
              </InsetGroup>
            </YStack>
          ) : null}
          <InsetGroup testID="skin-new-tab-kinds">
            {ACTIONS.map(({ kind, title, icon }) => (
              <ListItem
                key={kind}
                variant="grouped"
                showChevron={kind === 'browser'}
                pressLog="off"
                accessibilityLabel={title}
                onPress={kind === 'browser' ? () => setBrowser(true) : then(() => onCreate(kind))}
                testID={`skin-new-tab-${kind}`}
              >
                <XStack items="center" gap="$3">
                  <Icon as={icon} size={18} color="$color11" />
                  <Text>{title}</Text>
                </XStack>
              </ListItem>
            ))}
          </InsetGroup>
        </YStack>
      )}
    </ScreenSheet>
  );
}
