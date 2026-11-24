import { fireEvent, renderScreen, waitFor, within } from '@lib/flow-test-harness';
import { Text } from '@lib/tamagui-design-system';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import { Pressable } from 'react-native';
import { describe, expect, it } from 'vitest';

const header = async (view: ReturnType<typeof renderScreen>) => (await waitFor(() => view.getByTestId('SlotsHeader'))) as HTMLElement;

describe('SheetHeader leading and trailing slots', () => {
  it('Given a header with a leading and a trailing control, When it renders, Then the title sits between them in that order', async () => {
    // Given a header with both slots
    const view = renderScreen(
      <Sheet open onOpenChange={() => {}} testID="SlotsSheet">
        <SheetHeader
          testID="SlotsHeader"
          leading={
            <Pressable testID="SlotsReset" accessibilityRole="button">
              <Text>Reset</Text>
            </Pressable>
          }
          trailing={
            <Pressable testID="SlotsDone" accessibilityRole="button">
              <Text>Done</Text>
            </Pressable>
          }
        >
          <Text>Symbol Bar</Text>
        </SheetHeader>
      </Sheet>
    );

    // Then both controls and the title are in the header, leading first and trailing last
    const root = await header(view);
    const reset = within(root).getByTestId('SlotsReset');
    const title = within(root).getByText('Symbol Bar');
    const done = within(root).getByTestId('SlotsDone');
    expect(reset.compareDocumentPosition(title) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(title.compareDocumentPosition(done) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('Given a header with only a leading control, When it renders, Then there is no trailing slot', async () => {
    // Given a header with one slot
    const view = renderScreen(
      <Sheet open onOpenChange={() => {}} testID="SlotsSheet">
        <SheetHeader
          testID="SlotsHeader"
          leading={
            <Pressable testID="SlotsReset" accessibilityRole="button">
              <Text>Reset</Text>
            </Pressable>
          }
        >
          <Text>Symbol Bar</Text>
        </SheetHeader>
      </Sheet>
    );

    // Then the header holds the leading control and the title only
    const root = await header(view);
    expect(within(root).getByTestId('SlotsReset')).toBeTruthy();
    expect(root.children).toHaveLength(2);
  });

  it('Given a pressed leading control, When it is pressed, Then its own handler runs and the sheet stays open', async () => {
    // Given a header whose leading control counts presses
    let presses = 0;
    let closed = 0;
    const view = renderScreen(
      <Sheet open onOpenChange={(open) => !open && closed++} testID="SlotsSheet">
        <SheetHeader
          testID="SlotsHeader"
          leading={
            <Pressable testID="SlotsReset" accessibilityRole="button" onPress={() => presses++}>
              <Text>Reset</Text>
            </Pressable>
          }
        >
          <Text>Symbol Bar</Text>
        </SheetHeader>
      </Sheet>
    );

    // When it is pressed
    fireEvent.click(await waitFor(() => view.getByTestId('SlotsReset')));

    // Then the handler ran once and nothing closed
    expect(presses).toBe(1);
    expect(closed).toBe(0);
  });

  it('Given slots and a close control, When it renders, Then the close X is still last and closes the sheet', async () => {
    // Given a header with a trailing control and a close X
    let closed = 0;
    const view = renderScreen(
      <Sheet open onOpenChange={() => {}} testID="SlotsSheet">
        <SheetHeader
          testID="SlotsHeader"
          onClose={() => closed++}
          trailing={
            <Pressable testID="SlotsDone" accessibilityRole="button">
              <Text>Done</Text>
            </Pressable>
          }
        >
          <Text>Symbol Bar</Text>
        </SheetHeader>
      </Sheet>
    );

    // When the close X is pressed
    const root = await header(view);
    const close = within(root).getByTestId('SlotsHeaderClose');
    expect(root.lastElementChild).toBe(close);
    fireEvent.click(close);

    // Then the header's own close handler ran
    expect(closed).toBe(1);
  });

  it('Given no slots, When the header renders, Then it holds the title and the close X as before', async () => {
    // Given a header with neither slot
    const view = renderScreen(
      <Sheet open onOpenChange={() => {}} testID="SlotsSheet">
        <SheetHeader testID="SlotsHeader" onClose={() => {}}>
          <Text>Plain</Text>
        </SheetHeader>
      </Sheet>
    );

    // Then there are two children and no slot wrapper
    const root = await header(view);
    expect(root.children).toHaveLength(2);
    expect(within(root).getByTestId('SlotsHeaderClose')).toBe(root.lastElementChild);
  });
});
