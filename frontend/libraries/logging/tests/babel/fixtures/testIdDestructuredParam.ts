import { Logger } from '@lib/logging';

function IconButtonLike({ testID, onPress }: { testID: string; onPress: () => void }) {
  function handlePress() {
    Logger.info('ds.iconButtonLike.press');
    onPress();
  }
  return handlePress;
}
