import { Logger } from '@lib/logging';

function ButtonLike({ variant, ...props }: { variant: string; testID?: string }) {
  const handlePress = () => {
    Logger.info('ds.buttonLike.press', { variant });
  };
  return handlePress;
}
