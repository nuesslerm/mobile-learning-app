import { useComposerOptions, type ComposerOptionsProps } from '@lib/skin';
import { Button, Icon, InsetGroup, ListItem, Text } from '@lib/tamagui-design-system';
import { Check, ChevronLeft, FilePen, Hand, ShieldAlert, ShieldCheck } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { ScrollView } from 'react-native';

import { ScreenSheet } from '../sheet/ScreenSheet';

const PERMISSION_ICONS = {
  'approval-required': Hand,
  'auto-accept-edits': FilePen,
  auto: ShieldCheck,
  'full-access': ShieldAlert,
} as const;

/** The composer's model, effort and permission choices: a pill that names the model and opens a sheet of list rows. */
export function ComposerOptions(props: ComposerOptionsProps) {
  const { page, setPage, error, busy, load, returnToModels, select, permission, label, choices, selected } = useComposerOptions(props);
  const fullAccess = props.permission === 'full-access';
  const close = () => props.onVisibleChange(false);
  return (
    <>
      <Button
        variant="secondary"
        glass={false}
        size="$3"
        rounded="$10"
        maxW={180}
        accessibilityLabel={`Model, ${label}${fullAccess ? ', full access' : ''}`}
        pressLog="off"
        onPress={() => props.onVisibleChange(true)}
        testID="skin-composer-options"
      >
        {fullAccess ? <Icon as={ShieldAlert} size={15} color="$yellow9" /> : null}
        {label}
      </Button>
      {props.visible ? (
        <ScreenSheet onClose={close} snapPoints={[70]} handleLabel="Close" testID="skin-composer-options-sheet">
          <YStack gap="$3" pb="$4" flex={1}>
            <XStack items="center" px="$4" gap="$2">
              {page !== 'Model' ? (
                <XStack role="button" aria-label="Back to models" p="$1" pressStyle={{ opacity: 0.7 }} cursor="pointer" onPress={returnToModels}>
                  <Icon as={ChevronLeft} size={20} />
                </XStack>
              ) : null}
              <Text variant="large" role="heading" flex={1}>
                {page}
              </Text>
              <Button variant="ghost" glass={false} size="$3" pressLog="off" onPress={close} testID="skin-composer-options-done">
                Done
              </Button>
            </XStack>
            <ScrollView bounces={false} keyboardShouldPersistTaps="handled" contentContainerStyle={{ gap: 12 }}>
              {page === 'Model' ? (
                <Text variant="muted" textTransform="uppercase" letterSpacing={0.5} mx="$4">
                  Model
                </Text>
              ) : null}
              {choices && choices.length > 0 ? (
                <InsetGroup testID="skin-composer-options-choices">
                  {choices.map((choice) => {
                    const icon = page === 'Permissions' ? PERMISSION_ICONS[choice.slug as keyof typeof PERMISSION_ICONS] : undefined;
                    const configured = page === 'Model' && choice.slug === 'default';
                    return (
                      <ListItem
                        key={choice.slug}
                        variant="grouped"
                        pressLog="off"
                        accessibilityLabel={choice.name}
                        selected={selected === choice.slug}
                        disabled={busy}
                        onPress={() => select(choice.slug)}
                        testID="skin-composer-options-choice"
                      >
                        <XStack items="center" gap="$3">
                          {icon ? <Icon as={icon} size={18} color={choice.slug === 'full-access' ? '$yellow9' : '$color11'} /> : null}
                          <YStack flex={1} gap="$1">
                            <Text variant={configured ? 'strong' : 'default'}>{choice.name}</Text>
                            {configured ? <Text variant="muted">The model {props.provider} is configured with</Text> : null}
                          </YStack>
                          {selected === choice.slug ? <Icon as={Check} size={17} /> : null}
                        </XStack>
                      </ListItem>
                    );
                  })}
                </InsetGroup>
              ) : null}
              {choices?.length === 0 ? <Text mx="$4">No models available</Text> : null}
              {!choices && !error ? <Text mx="$4">Loading models…</Text> : null}
              {error ? (
                <Text role="alert" mx="$4">
                  {error}
                </Text>
              ) : null}
              {error && !choices ? (
                <Button variant="ghost" glass={false} self="flex-start" mx="$2" pressLog="off" onPress={load} testID="skin-composer-options-retry">
                  Try again
                </Button>
              ) : null}
              {page === 'Model' && permission ? (
                <YStack gap="$2">
                  <Text variant="muted" textTransform="uppercase" letterSpacing={0.5} mx="$4">
                    Options
                  </Text>
                  <InsetGroup testID="skin-composer-options-more">
                    <ListItem variant="grouped" showChevron pressLog="off" accessibilityLabel="Permissions" onPress={() => setPage('Permissions')} testID="skin-composer-options-permissions">
                      <XStack items="center" gap="$3">
                        <Text flex={1}>Permissions</Text>
                        <Text color={permission.slug === 'full-access' ? '$yellow9' : '$color11'}>{permission.name}</Text>
                      </XStack>
                    </ListItem>
                  </InsetGroup>
                </YStack>
              ) : null}
            </ScrollView>
          </YStack>
        </ScreenSheet>
      ) : null}
    </>
  );
}
