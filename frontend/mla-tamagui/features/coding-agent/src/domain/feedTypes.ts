import type {
  OrchestrationMessage,
  OrchestrationProposedPlan,
  OrchestrationThreadActivity,
} from './orchestration';

export type WorkLogKind =
  | 'command'
  | 'file_change'
  | 'file_read'
  | 'web_search'
  | 'mcp'
  | 'tool'
  | 'diff';

export type WorkLogStatus = 'running' | 'done' | 'failed' | 'declined';

export interface WorkLogEntry {
  id: string;
  kind: WorkLogKind;
  title: string;
  detail?: string;
  status: WorkLogStatus;
  diff?: string;
  output?: string;
}

export type ThreadFeedEntry =
  | {
      kind: 'user-message';
      id: string;
      turnId?: string;
      message: OrchestrationMessage;
    }
  | {
      kind: 'assistant-message';
      id: string;
      turnId?: string;
      message: OrchestrationMessage;
    }
  | {
      kind: 'work-log';
      id: string;
      turnId?: string;
      entry: WorkLogEntry;
    }
  | {
      kind: 'work-toggle';
      id: string;
      turnId?: string;
      groupId: string;
      hiddenCount: number;
    }
  | {
      kind: 'approval';
      id: string;
      turnId?: string;
      activity: OrchestrationThreadActivity;
    }
  | {
      kind: 'user-input';
      id: string;
      turnId?: string;
      activity: OrchestrationThreadActivity;
    }
  | {
      kind: 'proposed-plan';
      id: string;
      turnId?: string;
      plan: OrchestrationProposedPlan;
    }
  | {
      kind: 'error';
      id: string;
      turnId?: string;
      activity: OrchestrationThreadActivity;
    }
  | {
      kind: 'working';
      id: string;
      turnId?: string;
    };
