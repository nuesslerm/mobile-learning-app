import {
  useAgentComponents,
  type OnboardingChoiceProps,
} from '@lib/agent-ui-contracts';
import { ChatUiSettingsScreen } from '../ui/settings/ChatUiSettingsScreen';
import { createFlowApp, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { useState } from 'react';
import { Pressable, Text } from 'react-native';
import { describe, expect, it } from 'vitest';

import {
  CodingAgentContainer,
  CODING_AGENT_OVERRIDES,
  CODING_AGENT_UI_OVERRIDES,
} from './CodingAgentContainer';
import { createFixturePorts } from '../testing/fixturePorts';

function Choices() {
  const { NotificationChoices } = useAgentComponents();
  const [selected, select] = useState<'enable' | 'skip' | null>(null);
  return selected ? (
    <Text>{selected}</Text>
  ) : (
    <NotificationChoices
      enableLabel="Enable notifications"
      enableAccessibilityLabel="Enable agent notifications"
      skipLabel="Not now"
      skipAccessibilityLabel="Skip notifications for now"
      busyChoice={null}
      disabled={false}
      onChoice={select}
    />
  );
}

function TestRoute() {
  return (
    <CodingAgentContainer>
      <Choices />
    </CodingAgentContainer>
  );
}

function Replacement({
  accessibilityLabel,
  onPress,
  label,
}: OnboardingChoiceProps) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      onPress={onPress}
    >
      <Text>{label} (replacement)</Text>
    </Pressable>
  );
}

const app = createFlowApp({
  routes: { '../routes/index.tsx': { default: TestRoute } },
  routesRoot: '../routes/',
  basePath: '/coding-agent',
});

describe('Notification choices in the coding-agent container', () => {
  it.each([false, true])(
    'Given a nested choice override of %s, when skipping notifications, then the same action completes',
    async (replace) => {
      await app.render('/coding-agent', {
        overrides: {
          [CODING_AGENT_OVERRIDES.name]: createFixturePorts(),
          [CODING_AGENT_UI_OVERRIDES.name]: replace
            ? { OnboardingChoice: Replacement }
            : {},
        },
      });
      expect(
        screen.getByText(replace ? 'Not now (replacement)' : 'Not now')
      ).toBeTruthy();
      await userEvent.click(
        screen.getByRole('button', { name: 'Skip notifications for now' })
      );
      expect(screen.getByText('skip')).toBeTruthy();
    }
  );
});

it('Given two mounted Expo route stacks, when one changes the device chat font, both immediately display the new font', async () => {
  function Stacks() {
    return <>
      <CodingAgentContainer><ChatUiSettingsScreen /></CodingAgentContainer>
      <CodingAgentContainer><ChatUiSettingsScreen /></CodingAgentContainer>
    </>;
  }
  const stacks = createFlowApp({
    routes: { '../routes/index.tsx': {default: Stacks} },
    routesRoot: '../routes/',
    basePath: '/coding-agent',
  });
  await stacks.render('/coding-agent');
  await userEvent.click(screen.getAllByRole('button', {name: 'Font, System'})[0]);
  await userEvent.click(await screen.findByRole('button', {name: 'Serif'}));
  await waitFor(() => expect(screen.getAllByRole('button', {name: 'Font, Serif'})).toHaveLength(2));
  await userEvent.click(screen.getAllByRole('button', {name:'Reset text settings'})[1]);
  await waitFor(() => expect(screen.getAllByRole('button', {name: 'Font, System'})).toHaveLength(2));
});
