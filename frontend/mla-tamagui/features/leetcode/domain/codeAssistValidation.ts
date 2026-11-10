import type { ToolCall } from './codeAssistModels';

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

export function validateToolCall(doc: string, tool: ToolCall): boolean {
  switch (tool.tool) {
    case 'replaceRange':
      return (
        tool.params.from >= 0 &&
        tool.params.to <= doc.length &&
        tool.params.from <= tool.params.to &&
        tool.params.text.length <= 200
      );
    case 'insertAt':
      return (
        tool.params.position >= 0 &&
        tool.params.position <= doc.length &&
        tool.params.text.length <= 200
      );
    case 'deleteRange':
      return (
        tool.params.from >= 0 &&
        tool.params.to <= doc.length &&
        tool.params.from <= tool.params.to &&
        tool.params.to - tool.params.from <= 200
      );
    case 'renameSymbol':
      return (
        tool.params.oldName.length > 0 &&
        tool.params.oldName.length <= 100 &&
        tool.params.newName.length > 0 &&
        tool.params.newName.length <= 100 &&
        !new RegExp(`\\b${escapeRegex(tool.params.newName)}\\b`).test(doc)
      );
    default:
      return false;
  }
}
