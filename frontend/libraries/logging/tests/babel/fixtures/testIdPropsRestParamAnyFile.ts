import { Logger } from '@lib/logging';

function ScreenLike({ variant, ...props }: { variant: string; testID?: string }) {
  const handlePress = () => {
    Logger.info('screenLike.press', { variant });
  };
  return handlePress;
}
