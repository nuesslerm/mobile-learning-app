import {
  Button,
  ListItem,
  Text,
  TextField,
  View,
} from '@lib/tamagui-design-system';
import {
  Sheet,
  SheetHeader,
} from '@lib/tamagui-design-system/ui/sheet';
import {
  BottomSheet as ExpoBottomSheet,
  Button as ExpoButton,
  Column,
  Host,
  Text as ExpoText,
} from '@expo/ui';
import { useState } from 'react';

import { Compare } from '../Compare';

type Example = 'default' | 'compact' | 'no-backdrop' | 'scroll' | 'form';

const ROWS = Array.from({ length: 24 }, (_, i) => `Row ${i + 1}`);

/** Bottom sheet examples: each button opens one DS Sheet configuration; the Expo UI sheet is the platform's own. */
export function SheetExamples() {
  const [open, setOpen] = useState<Example | null>(null);
  const [expoOpen, setExpoOpen] = useState(false);
  const [name, setName] = useState('');
  const close = () => setOpen(null);

  return (
    <Compare
      testID="ShowcaseSheet"
      tamagui={
        <>
          <Button
            variant="outline"
            testID="ShowcaseSheetOpenDefault"
            onPress={() => setOpen('default')}
          >
            Default: two snap points
          </Button>
          <Button
            variant="outline"
            testID="ShowcaseSheetOpenCompact"
            onPress={() => setOpen('compact')}
          >
            Compact: one snap point, labelled handle
          </Button>
          <Button
            variant="outline"
            testID="ShowcaseSheetOpenNoBackdrop"
            onPress={() => setOpen('no-backdrop')}
          >
            No backdrop: the page stays usable
          </Button>
          <Button
            variant="outline"
            testID="ShowcaseSheetOpenScroll"
            onPress={() => setOpen('scroll')}
          >
            Scrolling body
          </Button>
          <Button
            variant="outline"
            testID="ShowcaseSheetOpenForm"
            onPress={() => setOpen('form')}
          >
            With a text field
          </Button>

          <Sheet
            open={open === 'default'}
            onOpenChange={(next) => !next && close()}
            snapPoints={[85, 50]}
            testID="ShowcaseSheetDefault"
          >
            <SheetHeader onClose={close} testID="ShowcaseSheetDefaultHeader"><Text variant="large">Default</Text></SheetHeader>
            <View p="$4" gap="$2">
              <Text>Opens at the lower point and drags up to the larger one.</Text>
              <Text variant="muted">
                Swipe down or tap the dimmed layer to dismiss.
              </Text>
            </View>
          </Sheet>

          <Sheet
            open={open === 'compact'}
            onOpenChange={(next) => !next && close()}
            snapPoints={[40]}
            handleLabel="Close the compact sheet"
            testID="ShowcaseSheetCompact"
          >
            <SheetHeader testID="ShowcaseSheetCompactHeader"><Text variant="large">Compact</Text></SheetHeader>
            <View p="$4">
              <Text>
                The handle is a button, so a screen reader can dismiss it.
              </Text>
            </View>
          </Sheet>

          <Sheet
            open={open === 'no-backdrop'}
            onOpenChange={(next) => !next && close()}
            snapPoints={[35]}
            backdrop="none"
            testID="ShowcaseSheetNoBackdrop"
          >
            <SheetHeader
              onClose={close}
              testID="ShowcaseSheetNoBackdropHeader"
            ><Text variant="large">No backdrop</Text></SheetHeader>
            <View p="$4">
              <Text>
                Touches go through to the page behind, which stays usable.
              </Text>
            </View>
          </Sheet>

          <Sheet
            open={open === 'scroll'}
            onOpenChange={(next) => !next && close()}
            snapPoints={[85]}
            testID="ShowcaseSheetScroll"
          >
            <SheetHeader onClose={close} testID="ShowcaseSheetScrollHeader"><Text variant="large">Scrolling body</Text></SheetHeader>
            <Sheet.ScrollView>
              {ROWS.map((row) => (
                <ListItem key={row} testID={`ShowcaseSheetScrollRow-${row}`}>
                  <Text>{row}</Text>
                </ListItem>
              ))}
            </Sheet.ScrollView>
          </Sheet>

          <Sheet
            open={open === 'form'}
            onOpenChange={(next) => !next && close()}
            snapPoints={[95]}
            testID="ShowcaseSheetForm"
          >
            <SheetHeader onClose={close} testID="ShowcaseSheetFormHeader"><Text variant="large">With a text field</Text></SheetHeader>
            <View p="$4">
              <TextField
                label="Name"
                placeholder="Ada Lovelace"
                value={name}
                onChangeText={setName}
                testID="ShowcaseSheetFormField"
              />
            </View>
          </Sheet>
        </>
      }
      expoUi={
        <Host matchContents>
          <ExpoButton
            label="Open Expo UI sheet"
            variant="outlined"
            onPress={() => setExpoOpen(true)}
            testID="ShowcaseExpoSheetOpen"
          />
          <ExpoBottomSheet
            isPresented={expoOpen}
            onDismiss={() => setExpoOpen(false)}
            snapPoints={['half', 'full']}
            testID="ShowcaseExpoSheet"
          >
            <Column>
              <ExpoText>The platform's own bottom sheet.</ExpoText>
              <ExpoButton
                label="Close"
                onPress={() => setExpoOpen(false)}
                testID="ShowcaseExpoSheetClose"
              />
            </Column>
          </ExpoBottomSheet>
        </Host>
      }
    />
  );
}
