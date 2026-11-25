import {
  useAgentComponents,
  type OnboardingChoiceProps,
} from '@lib/agent-ui-contracts';
import { createFlowApp, screen, userEvent } from '@lib/flow-test-harness';
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
