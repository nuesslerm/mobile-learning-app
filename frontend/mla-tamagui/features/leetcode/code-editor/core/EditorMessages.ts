export type EditorConfig = {
  initialText: string;
  isDark: boolean;
  fontSize: number;
  lineHeight: number;
  wordWrap: boolean;
  language: string;
  cursorColor: string;
  selectionColor: string;
  backgroundColor: string;
  gutterColor: string;
  borderColor: string;
  tabSize: number;
  noAutoFocus?: boolean;
};

export type ToEditorMessage =
  | { type: 'init'; config: EditorConfig }
  | { type: 'updateConfig'; config: Partial<EditorConfig> }
  | { type: 'focus' }
  | { type: 'setSelection'; start: number; end: number }
  | { type: 'insertAt'; text: string; offset?: number }
  | { type: 'undo' }
  | { type: 'redo' }
  | { type: 'setText'; text: string }
  | { type: 'blur' }
  | { type: 'highlightRange'; start: number; end: number; durationMs?: number }
  | { type: 'clearHighlights' }
  | { type: 'showDiff'; start: number; end: number }
  | { type: 'hideDiff' };
