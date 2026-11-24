import { Button, Icon, InsetGroup, Text, View } from '@lib/tamagui-design-system';
import { Brain, Check, ChevronDown, Clock, ListTodo, Zap } from '@lib/tamagui-design-system/icons';
import { Theme, XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { Link } from 'expo-router';
import { useState } from 'react';
import { Platform } from 'react-native';

import { Overlay } from '../overlayHost';

const LENGTHS = [
  { minutes: 5, description: 'Review a few flashcards', icon: Zap },
  { minutes: 15, description: 'Practice one topic', icon: Clock },
  { minutes: 25, description: 'Work through a problem set', icon: ListTodo },
  { minutes: 45, description: 'Settle into a longer study session', icon: Brain },
] as const;
const LINK_COLOR = '#A4DBFF';
const SELECTED_COLOR = '#A2E3CE';
const LONG_SESSION_COLOR = '#F4C48B';

export function GlassPickerItem() {
  const [open, setOpen] = useState(false);
  const [minutes, setMinutes] = useState(15);

  return (
    <>
      <Button
        variant="outline"
        rounded="$7"
        justify="space-between"
        testID="ShowcaseOpenSessionLength"
        aria-expanded={open}
        onPress={() => setOpen(true)}
      >
        <XStack items="center" gap="$2">
          <Icon as={Clock} size={18} />
          <Text>Session length</Text>
        </XStack>
        <XStack items="center" gap="$2">
          <Text variant="muted" testID="ShowcaseSessionLengthValue">{minutes} minutes</Text>
          <Icon as={ChevronDown} size={16} color="$color11" />
        </XStack>
      </Button>
      <Overlay id="showcase-glass-picker">
        <Theme name="dark">
          <Sheet
            accessible={false}
            floating
            floatingBottomInset={10}
            glassColorScheme="dark"
            glassTintColor="rgba(23, 23, 24, 0.56)"
            open={open}
            onOpenChange={setOpen}
            snapPoints={[47]}
            testID="ShowcaseSessionLengthSheet"
          >
            <XStack items="center" justify="space-between" pl="$2" gap="$3">
              <Text variant="h3" role="heading" flex={1}>Session length</Text>
              <Button
                variant="outline"
                borderColor="rgba(255, 255, 255, 0.24)"
                rounded="$12"
                size="$3"
                testID="ShowcaseCloseSessionLength"
                onPress={() => setOpen(false)}
              >
                Done
              </Button>
            </XStack>
            <Sheet.ScrollView>
              <View
                {...(Platform.OS === 'web' ? { role: 'radiogroup', 'aria-label': 'Session length' } : {})}
                bg="rgba(255, 255, 255, 0.06)"
                rounded="$6"
                overflow="hidden"
              >
                <InsetGroup inset={false} surface="transparent" testID="ShowcaseSessionLengths">
                  {LENGTHS.map(({ minutes: length, description, icon }) => {
                    const selected = minutes === length;
                    const label = `${length} minutes`;

                    return (
                      <XStack
                        key={length}
                        role="radio"
                        aria-label={label}
                        aria-checked={selected}
                        {...(Platform.OS === 'ios' ? { accessible: true, accessibilityHint: description } : {})}
                        tabIndex={0}
                        testID={`ShowcaseSessionLength-${length}`}
                        items="center"
                        gap="$3"
                        minH={72}
                        px="$3"
                        py="$2"
                        bg={selected ? 'rgba(127, 220, 191, 0.14)' : 'transparent'}
                        pressStyle={{ bg: 'rgba(127, 127, 127, 0.22)' }}
                        cursor="pointer"
                        onPress={() => setMinutes(length)}
                      >
                        <View width={34} height={34} items="center" justify="center" rounded="$4" bg="rgba(255, 255, 255, 0.08)">
                          <Icon as={icon} size={17} color="$color11" />
                        </View>
                        <YStack flex={1}>
                          <Text color={selected ? SELECTED_COLOR : length === 45 ? LONG_SESSION_COLOR : '$color'}>{label}</Text>
                          <Text variant="muted">{description}</Text>
                        </YStack>
                        {selected && <Icon as={Check} size={19} />}
                      </XStack>
                    );
                  })}
                </InsetGroup>
              </View>
            </Sheet.ScrollView>
            <XStack items="center" flexWrap="wrap" justify="space-between" px="$2" pb="$5" gap="$2">
              <Link href="/" onPress={() => setOpen(false)}>
                <Text variant="small" size="$4" color={LINK_COLOR} textDecorationLine="underline">
                  Practice problems
                </Text>
              </Link>
              <Link href="https://lsc.cornell.edu/how-to-study/" target="_blank">
                <Text variant="small" size="$4" color={LINK_COLOR} textDecorationLine="underline">
                  Study guide
                </Text>
              </Link>
            </XStack>
          </Sheet>
        </Theme>
      </Overlay>
    </>
  );
}
