import {
  useSkin,
  type NotificationChoicesProps,
} from '@lib/skin';

export function NotificationChoices(props: NotificationChoicesProps) {
  const { OnboardingChoice: Choice } = useSkin();
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
