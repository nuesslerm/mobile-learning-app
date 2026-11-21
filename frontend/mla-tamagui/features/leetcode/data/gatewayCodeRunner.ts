import type { CodeRunnerPort, LeetCodeGatewayPort } from '../domain/ports';

/** The first runner: the leetcode-backend, reached through the gateway. */
export function createGatewayCodeRunner(gateway: LeetCodeGatewayPort): CodeRunnerPort {
  return { run: (params) => gateway.runLocal(params) };
}
