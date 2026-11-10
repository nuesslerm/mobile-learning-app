import { Logger } from '@lib/logging';
import { Platform, ScrollView } from 'react-native';

import { Copy } from '../icons';
import { XStack, YStack, useTheme } from '../primitives';

import { IconButton } from './icon-button';
import { Text } from './text';

export type CodeBlockProps = {
  code: string;
  /** Optional header text shown above the code. */
  title?: string;
  /** Optional language label rendered in the header row. */
  language?: string;
  /** When provided, a copy IconButton is rendered in the header. */
  onCopy?: (code: string) => void;
  testID?: string;
};

/**
 * Code block with monospace `Text`, header row (title + language + optional copy),
 * and a horizontal `ScrollView` for long lines.
 *
 * Built on Tamagui primitives (`XStack`/`YStack`) and the DS `Text` so a dark
 * theme switch recolours everything without a separate stylesheet.
 */
export function CodeBlock({
  code,
  title,
  language,
  onCopy,
  testID,
}: CodeBlockProps) {
  const theme = useTheme();
  const surface = theme.color3?.val ?? '#1c1c1e';
  const border = theme.borderColor?.val ?? 'rgba(128,128,128,0.35)';
  const muted = theme.color11?.val ?? 'rgba(160,160,160,1)';

  const handleCopy = () => {
    Logger.info('ds.codeblock.copy', { testID, language, length: code.length });
    onCopy?.(code);
  };

  const showHeader = Boolean(title || language || onCopy);

  return (
    <YStack
      testID={testID}
      rounded="$3"
      borderWidth={1}
      borderColor="$borderColor"
      bg="$color3"
      gap={0}
      style={{
        backgroundColor: surface,
        borderColor: border,
        overflow: 'hidden',
      }}
    >
      {showHeader ? (
        <CodeBlockHeader
          title={title}
          language={language}
          onCopy={onCopy ? handleCopy : undefined}
          testID={testID}
          mutedColor={muted}
        />
      ) : null}
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        contentContainerStyle={{ padding: 12 }}
      >
        {/* react-native-web's Text takes `selectable`, but this Text renders a plain DOM node on web */}
        <Text
          variant="code"
          {...(Platform.OS === 'web'
            ? { userSelect: 'text' as const }
            : { selectable: true })}
        >
          {code}
        </Text>
      </ScrollView>
    </YStack>
  );
}

function CodeBlockHeader({
  title,
  language,
  onCopy,
  testID,
  mutedColor,
}: {
  title?: string;
  language?: string;
  onCopy?: () => void;
  testID?: string;
  mutedColor: string;
}): React.ReactElement {
  return (
    <XStack
      items="center"
      justify="space-between"
      gap="$2"
      px="$3"
      py="$2"
      borderBottomWidth={1}
      borderColor="$borderColor"
    >
      <XStack items="center" gap="$2" flex={1}>
        {title ? (
          <Text size="$2" color="$color" numberOfLines={1}>
            {title}
          </Text>
        ) : null}
        {language ? (
          <Text
            size="$2"
            color="$color11"
            style={{ color: mutedColor }}
            numberOfLines={1}
          >
            {title ? `· ${language}` : language}
          </Text>
        ) : null}
      </XStack>
      {onCopy ? (
        <IconButton
          icon={Copy}
          iconSize={14}
          variant="ghost"
          size="$3"
          onPress={onCopy}
          testID={testID ? `${testID}-copy` : undefined}
          aria-label="Copy code"
        />
      ) : null}
    </XStack>
  );
}
