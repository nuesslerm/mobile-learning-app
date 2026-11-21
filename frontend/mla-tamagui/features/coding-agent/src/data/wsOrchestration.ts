import { resolveCodingAgentConfig } from './codingAgentConfig';
import { createWsGateway } from './wsGateway';
import type { OrchestrationPort, TokenStorePort } from '../domain/ports';

/** The live orchestration port: the WebSocket URL and bearer are resolved when the gateway is opened, not at import. */
export function createWsOrchestration(tokenStore: TokenStorePort): OrchestrationPort {
  return {
    transport: 'ws',
    async open() {
      return createWsGateway(await resolveCodingAgentConfig(tokenStore));
    },
  };
}
