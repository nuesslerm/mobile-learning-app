import { Spinner, Text, View } from '@lib/tamagui-design-system';

export function WorkingIndicator(): React.ReactElement {
  return (
    <View testID="WorkingIndicator" items="center" gap="$1.5" py="$1">
      <Spinner size="small" />
      <Text variant="muted">Working…</Text>
    </View>
  );
}