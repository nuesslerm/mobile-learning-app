import { Logger } from '@lib/logging';

function handleFail(err: unknown) {
  Logger.withError(err).error('agent.chat.fail');
}
