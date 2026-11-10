import { Logger } from '@lib/logging';

function ScreenWithEffect() {
  useEffect(() => {
    function onTimeout() {
      Logger.info('agent.chat.timeout');
    }
    setTimeout(onTimeout, 1000);
  }, []);
}
