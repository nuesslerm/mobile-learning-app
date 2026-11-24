import {
  Button,
  IconButton,
  InsetGroup,
  ListItem,
  Text,
  TextField,
  View,
  useHeaderHeightStore,
} from '@lib/tamagui-design-system';
import { X } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import {
  Sheet,
  usePublishSheetOpen,
} from '@lib/tamagui-design-system/ui/sheet';
import {
  BottomSheet as ExpoBottomSheet,
  Button as ExpoButton,
  Column,
  RNHostView,
  Row,
  ScrollView as ExpoScrollView,
  Spacer,
  Text as ExpoText,
} from '@expo/ui';
import { useState } from 'react';
import { Platform } from 'react-native';

import { Compare, ExpoHost } from '../Compare';
import { Overlay } from '../overlayHost';
import {
  expoSheetFill,
  expoSheetModifiers,
  expoSheetWidth,
} from './expoSheetModifiers';

type Example = {
  id: 'Default' | 'Compact' | 'NoBackdrop' | 'Scroll' | 'Form';
  label: string;
  title: string;
  heights: number[];
  description?: string;
};

const EXAMPLES: Example[] = [
  {
    id: 'Default',
    label: 'Two snap points',
    title: 'Two snap points',
    heights: [85, 50],
    description: 'Drag the handle to switch between the two heights.',
  },
  {
    id: 'Compact',
    label: 'Compact sheet',
    title: 'Compact sheet',
    heights: [40],
    description: 'A single height for a short message or a quick action.',
  },
  {
    id: 'NoBackdrop',
    label: 'Keep the page interactive',
    title: 'Interactive background',
    heights: [35],
    description: 'Try the item picker on the page behind this sheet.',
  },
  {
    id: 'Scroll',
    label: 'Scrolling body',
    title: 'Scrolling body',
    heights: [85],
  },
  {
    id: 'Form',
    label: 'With a text field',
    title: 'With a text field',
    heights: [95],
  },
];

const ROWS = Array.from({ length: 24 }, (_, index) => `Row ${index + 1}`);
const SHEET_CONTROL_FILL = 'rgba(127, 127, 127, 0.14)';

function ExampleHeader({
  title,
  close,
  testID,
}: {
  title: string;
  close: () => void;
  testID: string;
}) {
  return (
    <XStack items="center" gap="$3" px="$2" testID={testID}>
      <Text variant="h3" flex={1} role="heading">
        {title}
      </Text>
      <IconButton
        icon={X}
        accessibilityLabel="Close"
        bg={SHEET_CONTROL_FILL}
        onPress={close}
        testID={`${testID}Close`}
      />
    </XStack>
  );
}

function NameForm({ testID }: { testID: string }) {
  const [name, setName] = useState('');
  return (
    <TextField
      label="Name"
      placeholder="Ada Lovelace"
      placeholderTextColor="$color11"
      bg={SHEET_CONTROL_FILL}
      borderColor="rgba(127, 127, 127, 0.22)"
      rounded="$5"
      focusStyle={{ bg: SHEET_CONTROL_FILL, borderColor: '$color8' }}
      value={name}
      onChangeText={setName}
      testID={testID}
    />
  );
}

function ExampleRows({ testID }: { testID: string }) {
  return (
    <InsetGroup surface="transparent" testID={`${testID}Rows`}>
      {ROWS.map((row) => (
        <ListItem key={row} variant="grouped" testID={`${testID}Row-${row}`}>
          <Text>{row}</Text>
        </ListItem>
      ))}
    </InsetGroup>
  );
}

function ExampleBody({
  example,
  testID,
}: {
  example: Example;
  testID: string;
}) {
  return (
    <View px="$2" py="$3">
      {example.id === 'Form' ? (
        <NameForm testID={`${testID}Field`} />
      ) : (
        <Text>{example.description}</Text>
      )}
    </View>
  );
}

function DesignSystemExample({
  example,
  close,
}: {
  example: Example;
  close: () => void;
}) {
  const compact = example.id === 'Compact';
  const keyboardOverlay = Platform.OS === 'ios' && example.id === 'Form';
  const testID = `ShowcaseSheet${example.id}`;
  const topInset = useHeaderHeightStore((state) => state.height);

  return (
    <Sheet
      open
      onOpenChange={(next) => !next && close()}
      snapPoints={example.heights}
      topInset={topInset}
      keyboardOverlay={keyboardOverlay}
      backdrop={example.id === 'NoBackdrop' ? 'none' : 'dim'}
      handleLabel={compact ? 'Close the compact sheet' : undefined}
      testID={testID}
    >
      <ExampleHeader
        title={example.title}
        close={close}
        testID={`${testID}Header`}
      />
      {example.id === 'Scroll' ? (
        <Sheet.ScrollView testID={`${testID}Body`}>
          <ExampleRows testID={testID} />
        </Sheet.ScrollView>
      ) : keyboardOverlay ? (
        <Sheet.ScrollView automaticallyAdjustKeyboardInsets>
          <ExampleBody example={example} testID={testID} />
        </Sheet.ScrollView>
      ) : (
        <ExampleBody example={example} testID={testID} />
      )}
    </Sheet>
  );
}

function ExpoExample({
  example,
  close,
}: {
  example: Example;
  close: () => void;
}) {
  usePublishSheetOpen(example.id !== 'NoBackdrop');

  return (
    <ExpoBottomSheet
      isPresented
      onDismiss={close}
      snapPoints={example.heights.map((height) => ({ fraction: height / 100 }))}
      modifiers={expoSheetModifiers(example.id === 'NoBackdrop')}
      contentPadding={{ top: 12, bottom: 12, left: 20, right: 20 }}
      testID="ShowcaseExpoSheet"
    >
      <Column spacing={12} modifiers={expoSheetWidth}>
        <Row
          alignment="center"
          spacing={12}
          style={{ width: '100%', height: 44, paddingHorizontal: 16 }}
          modifiers={expoSheetWidth}
        >
          <ExpoText textStyle={{ fontSize: 20, fontWeight: '600' }}>
            {example.title}
          </ExpoText>
          <Spacer flexible />
          <ExpoButton
            label="Close"
            variant="text"
            onPress={close}
            style={{ paddingHorizontal: 12, height: 44 }}
            testID="ShowcaseExpoSheetHeaderClose"
          />
        </Row>
        {example.id === 'Scroll' ? (
          <ExpoScrollView
            modifiers={[...(expoSheetFill ?? []), ...(expoSheetWidth ?? [])]}
            style={{ paddingHorizontal: 16, paddingBottom: 8 }}
            testID="ShowcaseExpoSheetBody"
          >
            <RNHostView matchContents={{ vertical: true }}>
              <ExampleRows testID="ShowcaseExpoSheet" />
            </RNHostView>
          </ExpoScrollView>
        ) : (
          <RNHostView matchContents={{ vertical: true }}>
            <ExampleBody example={example} testID="ShowcaseExpoSheet" />
          </RNHostView>
        )}
      </Column>
    </ExpoBottomSheet>
  );
}

export function SheetExamples() {
  const [open, setOpen] = useState<Example | null>(null);
  const [expoOpen, setExpoOpen] = useState<Example | null>(null);
  const expoExamples = EXAMPLES.filter(
    (example) => example.id !== 'NoBackdrop' || Platform.OS === 'ios'
  );

  return (
    <Compare
      testID="ShowcaseSheet"
      tamagui={
        <>
          {EXAMPLES.map((example) => (
            <Button
              key={example.id}
              variant="outline"
              testID={`ShowcaseSheetOpen${example.id}`}
              onPress={() => setOpen(example)}
            >
              {example.label}
            </Button>
          ))}
          <Overlay id="showcase-sheet-examples">
            {open ? (
              <DesignSystemExample
                key={open.id}
                example={open}
                close={() => setOpen(null)}
              />
            ) : null}
          </Overlay>
        </>
      }
      expoUi={
        <>
          <ExpoHost>
            {expoExamples.map((example) => (
              <ExpoButton
                key={example.id}
                label={example.label}
                variant="outlined"
                onPress={() => setExpoOpen(example)}
                testID={`ShowcaseExpoSheetOpen${example.id}`}
              />
            ))}
            {Platform.OS !== 'ios' ? (
              <ExpoText>
                Expo supports an interactive sheet background on iOS.
              </ExpoText>
            ) : null}
          </ExpoHost>
          {expoOpen ? (
            <ExpoExample
              key={expoOpen.id}
              example={expoOpen}
              close={() => setExpoOpen(null)}
            />
          ) : null}
        </>
      }
    />
  );
}
