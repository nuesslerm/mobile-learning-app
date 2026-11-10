import { Logger as Log } from '@lib/logging';

function AgentChatScreen() {
  // Documented limitation: the plugin only matches the literal identifier
  // name `Logger`, not an aliased import binding — so this is intentionally
  // left untouched.
  Log.info('agent.chat.load');
}
