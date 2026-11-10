import type { ShellInstance } from '@cli/harness-contracts';

declare global {
  interface Window {
    __shell: ShellInstance;
  }
}

export function registerShell(shell: ShellInstance): void {
  window.__shell = shell;
}
