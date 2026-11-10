import { Button, Text, View } from '@lib/tamagui-design-system';
import { router } from 'expo-router';
import { describe, expect, it } from 'vitest';

import { renderScreen, routerHarness, screen, userEvent } from '../index';

function Probe() {
  return (
    <View testID="probe">
      <Text>hello flow</Text>
      <Button testID="go" onPress={() => router.push('/somewhere?x=1')}>
        Go
      </Button>
    </View>
  );
}

describe('renderScreen', () => {
  it('given a design-system screen, when rendered, then real DOM with testIDs and text appears', () => {
    renderScreen(<Probe />);
    expect(screen.getByTestId('probe')).toBeTruthy();
    expect(screen.getByText('hello flow')).toBeTruthy();
  });

  it('given a button that navigates, when pressed, then the router harness records the push', async () => {
    const r = routerHarness();
    renderScreen(<Probe />);
    await userEvent.click(screen.getByTestId('go'));
    expect(r.pushes()).toEqual(['/somewhere?x=1']);
  });
});
