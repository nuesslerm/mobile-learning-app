import { z } from 'zod';
import type { Pairing } from '../mobile';

export const tabKind = z.enum(['terminal', 'browser', 'markdown']);
export type TabKind = z.infer<typeof tabKind>;
export const layoutSchema = z.object({
  checkout: z.string(),
  directory: z.string(),
  threadId: z.string(),
  active: z.string().nullable(),
  tabs: z.array(
    z.object({
      id: z.string(),
      resource: z.string(),
      title: z.string(),
      kind: tabKind,
    })
  ),
});
export type WorkspaceLayout = z.infer<typeof layoutSchema>;
export type WorkspaceTab = WorkspaceLayout['tabs'][number];
export type TabAction =
  | { action: 'create'; id: string; kind: TabKind; url?: string }
  | { action: 'select'; id: string | null }
  | { action: 'close'; id: string };
export type Note = { path: string; content: string };
export type NoteDraft = { previous: string; content: string };
export interface TerminalConnection {
  write(text: string): void;
  resize(cols: number, rows: number): void;
  close(): void;
}
export interface BrowserConnection {
  click(
    x: number,
    y: number,
    modifiers?: string[],
    button?: 'left' | 'right'
  ): void;
  wheel(dx: number, dy: number): void;
  reload(): void;
  text(text: string): void;
  key(key: string): void;
  navigate(url: string): void;
  history(delta: number): void;
  resize(width: number, height: number, mobile?: boolean): void;
  close(): void;
}
export type TabAgent = { id: string; name: string; model: string };
export interface WorkspaceTabsPort {
  terminalImage(
    pair: Pairing,
    checkout: string,
    jpeg: ArrayBuffer
  ): Promise<string>;
  transcribe(pair: Pairing, wav: ArrayBuffer): Promise<string>;
  agents(pair: Pairing): Promise<TabAgent[]>;
  startAgent(
    pair: Pairing,
    layout: WorkspaceLayout,
    agent: TabAgent
  ): Promise<string>;
  layout(
    pair: Pairing,
    checkout: string,
    action?: TabAction
  ): Promise<WorkspaceLayout>;
  read(pair: Pairing, checkout: string, path: string): Promise<Note>;
  save(
    pair: Pairing,
    checkout: string,
    path: string,
    draft: NoteDraft
  ): Promise<Note>;
  draft(
    pair: Pairing,
    checkout: string,
    path: string
  ): Promise<NoteDraft | null>;
  keepDraft(
    pair: Pairing,
    checkout: string,
    path: string,
    draft: NoteDraft | null
  ): Promise<void>;
  terminal(
    pair: Pairing,
    layout: WorkspaceLayout,
    tab: WorkspaceTab,
    output: (text: string, reset: boolean) => void,
    error: (message: string) => void
  ): Promise<TerminalConnection>;
  browser(
    pair: Pairing,
    layout: WorkspaceLayout,
    tab: WorkspaceTab,
    frame: (uri: string) => void,
    error: (message: string) => void,
    navigation?: (url: string, back: boolean, forward: boolean) => void,
    textFocus?: (editable: boolean) => void
  ): Promise<BrowserConnection>;
}
