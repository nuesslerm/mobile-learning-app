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
import { Host, List, ListItem as ExpoListItem } from '@expo/ui';

import { Compare } from '../Compare';

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
        <InsetGroup testID="ShowcaseGroup">
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
            <Text testID="RowThirdLabel">No chevron, not pressable</Text>
          </ListItem>
        </InsetGroup>
      }
      expoUi={
        <View height={200}>
          <Host style={{ flex: 1 }}>
            <List testID="ShowcaseExpoList">
              <ExpoListItem onPress={() => {}} supportingText="Opens settings">
                Settings
              </ExpoListItem>
              <ExpoListItem onPress={() => {}}>Second row</ExpoListItem>
              <ExpoListItem>Not pressable</ExpoListItem>
            </List>
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
