import type { SourceControlProps } from '@lib/skin';
import { Button, Icon, InsetGroup, Spinner, Text } from '@lib/tamagui-design-system';
import { Check } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';

import { ScreenSheet } from '../sheet/ScreenSheet';

const TALL = [70];
const CONTENT = { gap: 12, paddingBottom: 24 } as const;

/** The checkout's branches as a sheet of rows, the current one ticked. */
export function BranchSheet({ branches, current, truncated, error, close, retry, select }: NonNullable<SourceControlProps['picker']>) {
  return (
    <ScreenSheet onClose={close} snapPoints={TALL} handleLabel="Close branch picker" testID="skin-branches">
      <YStack flex={1} gap="$3">
        <Text variant="muted" textTransform="uppercase" letterSpacing={0.5} mx="$4">
          Switch Branch
        </Text>
        {branches ? (
          <Sheet.ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={CONTENT}>
            <InsetGroup testID="skin-branches-list">
              {branches.map((name) => (
                <XStack
                  key={name}
                  role="button"
                  accessible
                  aria-label={name}
                  aria-selected={current === name}
                  items="center"
                  gap="$3"
                  px="$4"
                  py="$3"
                  pressStyle={{ bg: '$color3' }}
                  cursor="pointer"
                  onPress={() => {
                    close();
                    select(name);
                  }}
                >
                  <YStack flex={1}>
                    <Text numberOfLines={1}>{name}</Text>
                    {name === current ? <Text variant="muted">current</Text> : null}
                  </YStack>
                  {name === current ? <Icon as={Check} size={17} /> : null}
                </XStack>
              ))}
            </InsetGroup>
            {truncated ? (
              <Text variant="muted" mx="$4">
                Branch list truncated.
              </Text>
            ) : null}
          </Sheet.ScrollView>
        ) : error ? (
          <Button variant="ghost" glass={false} self="flex-start" mx="$2" pressLog="off" onPress={retry} testID="skin-branches-retry">
            {error} Retry
          </Button>
        ) : (
          <Spinner />
        )}
      </YStack>
    </ScreenSheet>
  );
}
