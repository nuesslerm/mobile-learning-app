import { useAgentComponents, type NotificationChoicesProps } from '@lib/agent-ui-contracts';

export function NotificationChoices(props: NotificationChoicesProps) {
  const { OnboardingChoice: Choice } = useAgentComponents();
  return (
    <>
      <Choice
        label={props.enableLabel}
        accessibilityLabel={props.enableAccessibilityLabel}
        primary
        busy={props.busyChoice === 'enable'}
        disabled={props.disabled}
        onPress={() => props.onChoice('enable')}
      />
      <Choice
        label={props.skipLabel}
        accessibilityLabel={props.skipAccessibilityLabel}
        busy={props.busyChoice === 'skip'}
        disabled={props.disabled}
        onPress={() => props.onChoice('skip')}
      />
    </>
  );
}
