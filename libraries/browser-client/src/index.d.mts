export type ContainerOptions = {
  /** Start the daemon headless (no display needed). */
  headless?: boolean;
  /** Where the daemon writes screenshots it takes. */
  screenshotDir?: string;
};

/** The result of a daemon command: whatever the command returns, JSON-decoded. */
export type CommandResult = Record<string, unknown> & { result?: unknown };

export type Command = (
  args?: Record<string, unknown>
) => Promise<CommandResult>;

/**
 * Every daemon command (`openBrowser`, `navigate`, `eval`, `cookies`, `screenshot`, `tap`, `closeBrowser`, ...) is a method;
 * `startDaemon` and `stopDaemon` manage the daemon itself.
 */
export type Container = {
  startDaemon(options?: {
    browserExecutablePath?: string;
  }): Promise<{ url: string }>;
  stopDaemon(): Promise<void>;
} & { [command: string]: Command };

export function createContainer(options?: ContainerOptions): Container;
