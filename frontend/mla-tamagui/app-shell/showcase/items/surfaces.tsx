import {
  Card,
  Icon,
  InsetGroup,
  ListItem,
  StatTile,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Rocket, Settings } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import { Host, ListItem as ExpoListItem, Text as ExpoText } from '@expo/ui';

import { Compare } from '../Compare';
import {
  ShowcaseNativeList,
  showcaseNativeRowModifiers,
} from './ShowcaseNativeList';

export function CardItem() {
  return (
    <Compare
      testID="ShowcaseCard"
      tamagui={
        <>
          <Card tone="default" testID="Showcase-CodingAgent-CardDefault">
            <Text>default</Text>
          </Card>
          <Card tone="accent" testID="Showcase-CodingAgent-CardAccent">
            <Text>accent</Text>
          </Card>
          <Card tone="danger" testID="Showcase-CodingAgent-CardDanger">
            <Text>danger</Text>
          </Card>
          <Card tone="warning" testID="Showcase-CodingAgent-CardWarning">
            <Text>warning</Text>
          </Card>
        </>
      }
    />
  );
}

export function ListItemsItem() {
  return (
    <Compare
      testID="ShowcaseList"
      tamagui={
        <InsetGroup inset={false} surface="transparent" testID="ShowcaseGroup">
          <ListItem
            testID="RowSettings"
            variant="grouped"
            showChevron
            onPress={() => {}}
          >
            <XStack gap="$2" items="center">
              <Icon as={Settings} size={16} color="$color11" />
              <Text testID="RowSettingsLabel">Settings</Text>
            </XStack>
          </ListItem>
          <ListItem
            testID="RowSecond"
            variant="grouped"
            showChevron
            onPress={() => {}}
          >
            <Text testID="RowSecondLabel">Second row</Text>
          </ListItem>
          <ListItem testID="RowThird" variant="grouped">
            <Text testID="RowThirdLabel">Read-only row</Text>
          </ListItem>
        </InsetGroup>
      }
      expoUi={
        <View height={168}>
          <Host style={{ flex: 1 }}>
            <ShowcaseNativeList>
              <ExpoListItem
                modifiers={showcaseNativeRowModifiers}
                onPress={() => {}}
              >
                Settings
              </ExpoListItem>
              <ExpoListItem
                modifiers={showcaseNativeRowModifiers}
                onPress={() => {}}
              >
                Second row
              </ExpoListItem>
              <ExpoText modifiers={showcaseNativeRowModifiers}>
                Read-only row
              </ExpoText>
            </ShowcaseNativeList>
          </Host>
        </View>
      }
    />
  );
}

export function StatTileItem() {
  return (
    <Compare
      testID="ShowcaseStatTile"
      tamagui={<StatTile icon={Rocket} value="98%" label="Tests passing" />}
    />
  );
}
