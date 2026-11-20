import { act, renderScreen, waitFor } from '@lib/flow-test-harness';
import { TextField } from '@lib/tamagui-design-system';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { Textarea } from '@lib/tamagui-design-system/ui/Textarea';
import { afterEach, describe, expect, it } from 'vitest';

const KEYBOARD_HEIGHT = 300;

// the keyboard up, signalled the way the device harness does it
function setKeyboard(height: number) {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: height > 0, __MCP_KEYBOARD_MODE__: 'overlay', __MCP_KEYBOARD_HEIGHT__: height });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
}

// a field a sheet's author reaches through another component, not written in the sheet's own JSX
function NestedField() {
  return <TextField testID="NestedField" />;
}

const Scene = () => (
  <Sheet open onOpenChange={() => {}} testID="FieldsSheet">
    <Sheet.TextInput testID="OwnInput" />
    <TextField testID="PlainTextField" label="Token" />
    <Textarea testID="PlainTextarea" />
    <NestedField />
  </Sheet>
);

const inset = () => document.querySelector('[data-sheet="true"]')?.getAttribute('data-bottom-inset');

async function focusWithKeyboard(testID: string, view: ReturnType<typeof renderScreen>) {
  const field = (await waitFor(() => view.getByTestId(testID))) as HTMLElement;
  expect(inset()).toBe('0');
  await act(async () => {
    field.focus();
    setKeyboard(KEYBOARD_HEIGHT);
  });
  return field;
}

afterEach(() => act(async () => setKeyboard(0)));

describe('a text field inside a Sheet and the keyboard', () => {
  it.each(['OwnInput', 'PlainTextField', 'PlainTextarea', 'NestedField'])(
    'Given %s in an open sheet, When it takes focus and the keyboard rises, Then the sheet is lifted by the keyboard height and settles when it lets go',
    async (testID) => {
      // Given a sheet holding the field with the keyboard down
      const view = renderScreen(<Scene />);

      // When the field takes focus and the keyboard rises
      const field = await focusWithKeyboard(testID, view);

      // Then the sheet is lifted by the keyboard's height
      await waitFor(() => expect(inset()).toBe(String(KEYBOARD_HEIGHT)));

      // When the field lets go and the keyboard goes down
      await act(async () => {
        field.blur();
        setKeyboard(0);
      });

      // Then the sheet settles back
      await waitFor(() => expect(inset()).toBe('0'));
    }
  );

  it('Given a field outside any sheet, When it takes focus and the keyboard rises, Then it renders as before, a plain Tamagui field with no sheet to lift', async () => {
    // Given a field with no sheet around it
    const view = renderScreen(<TextField testID="LooseField" label="Name" />);

    // When it takes focus and the keyboard rises
    const field = view.getByTestId('LooseField') as HTMLElement;
    await act(async () => {
      field.focus();
      setKeyboard(KEYBOARD_HEIGHT);
    });

    // Then it keeps focus and no sheet exists to be lifted
    expect(document.activeElement).toBe(field);
    expect(document.querySelector('[data-sheet="true"]')).toBeNull();
  });
});
