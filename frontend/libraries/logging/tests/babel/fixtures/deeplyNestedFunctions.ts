import { Logger } from '@lib/logging';

function OuterScreen() {
  function middleHandler() {
    function innerCallback() {
      Logger.info('agent.chat.retry');
    }
    return innerCallback;
  }
  return middleHandler;
}
