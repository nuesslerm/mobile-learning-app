// features/coding-agent/src/state/__tests__/fakeGateway.ts
//
// In-memory `OrchestrationGateway` for the `threadDetailStore` state tests.
// `dispatch` records every command it receives into `dispatched` (and throws
// when `disconnected` is set, so a "gateway down" path can be exercised), and
// `push(threadId, item)` fans a stream item out to the thread's subscribers
// so a test can drive `snapshot → event` sequences exactly as the WS would.
// Per the contract, state tests inject a fake gateway; this is that fake.
import type {
  ClientOrchestrationCommand,
  OrchestrationCommandReceipt,
  OrchestrationSubscribeThreadItem,
  OrchestrationThread,
  OrchestrationThreadSummary,
  ServerProbeResult,
  ThreadId,
} from '@app/remote-chat-agent-contracts/orchestration';

import type {
  OrchestrationGateway,
  OrchestrationGatewayStatus,
  OrchestrationShellListener,
  OrchestrationStatusListener,
  OrchestrationThreadListener,
} from '../../data/orchestrationGateway';

export class FakeGateway implements OrchestrationGateway {
  dispatched: ClientOrchestrationCommand[] = [];
  disconnected = false;

  private threadListeners = new Map<ThreadId, Set<OrchestrationThreadListener>>();
  private statusListeners = new Set<OrchestrationStatusListener>();
  private currentStatus: OrchestrationGatewayStatus = 'open';

  async connect(): Promise<void> {
    this.currentStatus = 'open';
  }

  close(): void {
    this.currentStatus = 'closed';
  }

  status(): OrchestrationGatewayStatus {
    return this.currentStatus;
  }

  onStatus(cb: OrchestrationStatusListener): () => void {
    this.statusListeners.add(cb);
    return () => {
      this.statusListeners.delete(cb);
    };
  }

  async probe(): Promise<ServerProbeResult> {
    return {
      availableClis: ['opencode'],
      selectedCli: 'opencode',
      version: 'fake-1.0.0',
      providers: [],
    };
  }

  async listThreads(): Promise<OrchestrationThreadSummary[]> {
    return [];
  }

  async getThread(_id: string): Promise<OrchestrationThread> {
    throw new Error('FakeGateway.getThread is not implemented');
  }

  subscribeShell(_cb: OrchestrationShellListener): () => void {
    return () => undefined;
  }

  subscribeThread(
    threadId: string,
    cb: OrchestrationThreadListener,
    _afterSequence?: number,
  ): () => void {
    const id = threadId as ThreadId;
    const listeners =
      this.threadListeners.get(id) ?? new Set<OrchestrationThreadListener>();
    listeners.add(cb);
    this.threadListeners.set(id, listeners);
    return () => {
      listeners.delete(cb);
    };
  }

  async dispatch(
    cmd: ClientOrchestrationCommand,
  ): Promise<OrchestrationCommandReceipt> {
    this.dispatched.push(cmd);
    if (this.disconnected) {
      throw new Error('FakeGateway: gateway is disconnected');
    }
    return { commandId: cmd.commandId, status: 'accepted', events: [] };
  }

  push(threadId: ThreadId, item: OrchestrationSubscribeThreadItem): void {
    const listeners = this.threadListeners.get(threadId);
    if (listeners === undefined) return;
    for (const listener of listeners) {
      listener(item);
    }
  }
}
