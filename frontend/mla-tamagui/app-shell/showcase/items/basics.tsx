import {
  Badge,
  Button,
  Icon,
  IconButton,
  Separator,
  Spinner,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Rocket, Settings, Sparkles } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import { Text as ExpoText } from '@expo/ui';

import { ExpoButton } from './ExpoButton';
import { Compare, ExpoHost } from '../Compare';

export function TextItem() {
  return (
    <Compare
      testID="ShowcaseText"
      tamagui={
        <View gap="$1">
          <Text variant="h1">Heading 1</Text>
          <Text variant="h2">Heading 2</Text>
          <Text>Body text</Text>
          <Text variant="muted">Muted text</Text>
          <Text variant="small">SMALL LABEL</Text>
        </View>
      }
      expoUi={
        <ExpoHost>
          <ExpoText textStyle={{ fontSize: 28, fontWeight: 'bold' }}>
            Heading 1
          </ExpoText>
          <ExpoText textStyle={{ fontSize: 22, fontWeight: '600' }}>
            Heading 2
          </ExpoText>
          <ExpoText>Body text</ExpoText>
        </ExpoHost>
      }
    />
  );
}

export function BadgeItem() {
  return (
    <Compare
      testID="ShowcaseBadge"
      tamagui={
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
      }
    />
  );
}

export function ButtonItem() {
  return (
    <Compare
      testID="ShowcaseButton"
      tamagui={
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
      }
      expoUi={
        <ExpoHost>
          <ExpoButton
            label="Filled"
            variant="filled"
            testID="ExpoButtonFilled"
          />
          <ExpoButton
            label="Outlined"
            variant="outlined"
            testID="ExpoButtonOutlined"
          />
          <ExpoButton label="Text" variant="text" testID="ExpoButtonText" />
        </ExpoHost>
      }
    />
  );
}

export function IconButtonItem() {
  return (
    <Compare
      testID="ShowcaseIconButton"
      tamagui={
        <XStack gap="$2">
          <IconButton
            icon={Settings}
            accessibilityLabel="Settings"
            testID="ShowcaseIconButtonSettings"
          />
          <IconButton
            icon={Sparkles}
            variant="outline"
            accessibilityLabel="Sparkles"
            testID="ShowcaseIconButtonSparkles"
          />
        </XStack>
      }
    />
  );
}

export function IconItem() {
  return (
    <Compare
      testID="ShowcaseIcon"
      tamagui={
        <XStack gap="$3" items="center">
          <Icon as={Settings} size={16} color="$color11" />
          <Icon as={Rocket} size={24} color="$color11" />
          <Icon as={Sparkles} size={32} color="$color11" />
        </XStack>
      }
    />
  );
}

export function SeparatorItem() {
  return (
    <Compare
      testID="ShowcaseSeparator"
      tamagui={
        <View gap="$2">
          <Text>Above</Text>
          <Separator />
          <Text>Below</Text>
        </View>
      }
    />
  );
}

export function SpinnerItem() {
  return (
    <Compare
      testID="ShowcaseSpinner"
      tamagui={
        <XStack gap="$4" items="center">
          <Spinner size="small" accessibilityLabel="Loading, small" />
          <Spinner size="large" accessibilityLabel="Loading, large" />
        </XStack>
      }
    />
  );
}
