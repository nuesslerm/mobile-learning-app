import { useChatText, useSkin, type ChatTextPreviewProps, type HomeSettingsSectionTitleProps, type OptionSheetProps } from '@lib/skin';
import { Icon, InsetGroup, ListItem, Text, View } from '@lib/tamagui-design-system';
import { Check } from '@lib/tamagui-design-system/icons';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';

/** A caption above a group, as the app's Settings writes "PREFERENCES". */
export function HomeSettingsSectionTitle(props: HomeSettingsSectionTitleProps) {
  return (
    <Text variant="muted" textTransform="uppercase" letterSpacing={0.5} mx="$4">
      {props.children}
    </Text>
  );
}

// The screen's own gap spaces its groups.
export function HomeSettingsSpacer() {
  return null;
}

export function ChatTextPreview(props: ChatTextPreviewProps) {
  const { ChatMessageText } = useSkin();
  const { value } = useChatText();
  return (
    <View mx="$4" p="$3" gap="$3" rounded="$4" bg="$color2">
      <View self="flex-end" px="$3" py="$2" rounded="$4" bg="$color4">
        <ChatMessageText tone="user" text={props.question} />
      </View>
      <Text color="$color10" fontSize={value.size} lineHeight={(20 * value.size) / 14}>
        {props.activity}
      </Text>
      <ChatMessageText text={props.answer} copyCode={props.copyCode} />
    </View>
  );
}

export function OptionSheet(props: OptionSheetProps) {
  return (
    <Sheet open onOpenChange={(open) => !open && props.onClose()} snapPoints={[45]} handleLabel={props.dismissLabel} testID="skin-option-sheet">
      <View gap="$3" pb={props.bottomInset}>
        <Text variant="muted" role="heading" mx="$4">
          {props.title}
        </Text>
        <InsetGroup testID="skin-option-sheet-options">
          {props.options.map((option) => (
            <ListItem
              key={option.id}
              variant="grouped"
              pressLog="off"
              accessibilityLabel={option.label}
              selected={option.selected}
              onPress={() => {
                props.onSelect(option.id);
                props.onClose();
              }}
              testID={`skin-option-${option.id}`}
            >
              <View flexDirection="row" items="center" gap="$3" flex={1}>
                <View flex={1} gap="$1">
                  <Text variant="strong">{option.label}</Text>
                  {option.subtitle ? <Text variant="muted">{option.subtitle}</Text> : null}
                </View>
                {option.selected ? <Icon as={Check} size={18} /> : null}
              </View>
            </ListItem>
          ))}
        </InsetGroup>
      </View>
    </Sheet>
  );
}
