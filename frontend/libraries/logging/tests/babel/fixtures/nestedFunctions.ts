import { Logger } from '@lib/logging';

function AgentChatScreen() {
  function handleRetry() {
    Logger.info('agent.chat.retry');
  }
  return handleRetry;
}
