import { Logger } from '@lib/logging';
import {
  FONT_PRESETS,
  type FontPresetConfig,
  Button,
  Icon,
  Separator,
  Text,
  View,
  useFontPresetStore,
} from '@lib/tamagui-design-system';
import { CircleCheck } from '@lib/tamagui-design-system/icons';
import * as React from 'react';
import { Pressable } from 'react-native';

type FontPresetsGridProps = {
  onApplied?: (preset: FontPresetConfig) => void;
};

export function FontPresetsGrid({ onApplied }: FontPresetsGridProps) {
  const activeFontId = useFontPresetStore((s) => s.activeFontId);
  const setActiveFont = useFontPresetStore((s) => s.setActiveFont);

  const [pendingId, setPendingId] = React.useState<string | null>(null);

  const handleTap = (preset: FontPresetConfig) => {
    // Tap selects (pending) but does NOT apply yet.
    const nextPending = preset.id === pendingId ? null : preset.id;
    Logger.info('fontPresets.tap', {
      fontId: preset.id,
      pending: nextPending ?? false,
    });
    setPendingId(nextPending);
  };

  const handleApply = () => {
    if (!pendingId) return;
    setActiveFont(pendingId);
    const applied = FONT_PRESETS.find((p) => p.id === pendingId);
    setPendingId(null);
    if (applied) {
      Logger.info('fontPresets.apply', {
        fontId: applied.id,
        fontName: applied.name,
      });
      onApplied?.(applied);
    }
  };

  return (
    <View gap="$3" testID="FontPresetsGrid" bg="$card" rounded="$lg">
      {FONT_PRESETS.map((preset) => {
        const isActive = (activeFontId ?? FONT_PRESETS[0].id) === preset.id;
        const isPending = pendingId === preset.id;
        const highlighted = isActive || isPending;

        return (
          <FontCard
            key={preset.id}
            preset={preset}
            isActive={isActive}
            isPending={isPending}
            highlighted={highlighted}
            onTap={() => {
              handleTap(preset);
            }}
          />
        );
      })}

      {pendingId && (
        <View pb="$4" testID="FontPresetsGrid-ApplyRow">
          <Separator testID="FontPresetsGrid-ApplySeparator" mb="$4" />
          <Button
            testID="FontPresetsGrid-ApplyBtn"
            onPress={handleApply}
            pressLog="off"
          >
            <Text
              testID="FontPresetsGrid-ApplyBtnLabel"
              color="$primary-foreground"
            >
              Apply Selected Font
            </Text>
          </Button>
        </View>
      )}
    </View>
  );
}

type FontCardProps = {
  preset: FontPresetConfig;
  isActive: boolean;
  isPending: boolean;
  highlighted: boolean;
  onTap: () => void;
};

function FontCard({
  preset,
  isActive,
  isPending,
  highlighted,
  onTap,
}: FontCardProps) {
  // Explicit `style` here overrides the globally active font (see
  // ui/text.tsx's mergedStyle — a passed `style` wins over the auto-applied
  // active-preset fontFamily), so each card previews ITS OWN typeface
  // regardless of which one is currently active app-wide.
  const previewStyle = preset.faces.regular
    ? { fontFamily: preset.faces.regular }
    : undefined;

  return (
    <Pressable
      testID={`FontCard-${preset.id}`}
      onPress={onTap}
      accessibilityRole="button"
      accessibilityLabel={`${preset.name} font${isActive ? ', currently active' : ''}`}
      accessibilityState={{ selected: highlighted }}
    >
      <View
        testID={`FontCard-Body-${preset.id}`}
        bg="$card"
        rounded="$xl"
        borderWidth={3}
        borderColor={highlighted ? '$primary' : '$border'}
        p="$4"
      >
        <View
          testID="FontCard-Header"
          mb="$3"
          flexDirection="row"
          items="flex-start"
          justify="space-between"
        >
          <View testID="FontCard-NameSection" flex={1} gap="$0.5">
            <Text
              testID={`FontCard-Name-${preset.id}`}
              color="$foreground"
              fontWeight="600"
            >
              {preset.name}
            </Text>
            <Text
              testID={`FontCard-Description-${preset.id}`}
              variant="small"
              color="$muted-foreground"
            >
              {preset.description}
            </Text>
          </View>

          {isActive && (
            <View
              testID={`FontCard-ActiveBadge-${preset.id}`}
              bg="$primary"
              ml="$2"
              flexDirection="row"
              items="center"
              gap="$1"
              rounded="$pill"
              px="$2.5"
              py="$1"
            >
              <Icon
                testID={`FontCard-ActiveIcon-${preset.id}`}
                as={CircleCheck}
                size={16}
                color="$primary-foreground"
              />
              <Text
                testID={`FontCard-ActiveLabel-${preset.id}`}
                variant="strong"
                color="$primary-foreground"
                fontSize={12}
              >
                ACTIVE
              </Text>
            </View>
          )}
          {!isActive && isPending && (
            <View
              testID={`FontCard-PendingBadge-${preset.id}`}
              borderColor="$primary"
              ml="$2"
              flexDirection="row"
              items="center"
              rounded="$pill"
              borderWidth={2}
              px="$2.5"
              py="$1"
            >
              <Text
                testID={`FontCard-PendingLabel-${preset.id}`}
                variant="strong"
                color="$primary"
                fontSize={12}
              >
                SELECTED
              </Text>
            </View>
          )}
        </View>

        <Text
          testID={`FontCard-Preview-${preset.id}`}
          color="$foreground"
          fontSize={28}
          style={previewStyle}
        >
          The quick brown fox jumps
        </Text>
      </View>
    </Pressable>
  );
}
