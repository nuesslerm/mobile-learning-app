import type { ComposerSendButtonProps } from '@lib/agent-ui-contracts';
import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { Pressable, Text } from 'react-native';
import { describe, expect, it } from 'vitest';
import { app } from '../../__tests__/legacyFlowApp';
import { CODING_AGENT_UI_OVERRIDES } from '../../di/CodingAgentContainer';
import { createMobileDouble } from '../../testing/mobilePort';

function ReplacementSend({ label, canSend, onPress }: ComposerSendButtonProps) {
  return <Pressable accessibilityRole="button" accessibilityLabel={label} disabled={!canSend} onPress={onPress}>
    <Text>Test send</Text>
  </Pressable>;
}

describe('Orca chat composer', () => {
  it.each([false, true])(
    'Given a paired thread with a nested send override of %s, when sending a draft, then the stream adds it to the transcript and the field clears',
    async replace => {
      const mobile = createMobileDouble({ endpoint: 'https://host.example', token: 'example' });
      await app.render('/coding-agent/thread-42', {
        search: { presentation: 'orca' },
        overrides: {
          'coding-agent': { mobile },
          [CODING_AGENT_UI_OVERRIDES.name]: replace ? { ComposerSendButton: ReplacementSend } : {},
        },
      });
      await screen.findByText('Check selection');
      const field = screen.getByPlaceholderText('Ask anything, @ to mention files, / for commands');
      await userEvent.type(field, 'Orca parity reply');
      if (replace) expect(screen.getByText('Test send')).toBeTruthy();
      await userEvent.click(screen.getByRole('button', { name: 'Send message' }));
      await waitFor(() => expect(screen.getByText('Orca parity reply')).toBeTruthy());
      expect((field as HTMLTextAreaElement).value).toBe('');
    }
  );
});
