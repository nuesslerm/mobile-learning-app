export declare const HOOK_EXIT: {
  readonly PROCEED: 0;
  readonly NOT_ENFORCING: 1;
  readonly BLOCK: 2;
};

export declare function failLoud(
  hookName: string,
  what: string,
  error?: unknown
): never;

export declare function proceed(): never;

export declare function block(message: string): never;

export declare function readHookInput(hookName: string): Record<string, unknown>;

export declare function runHook(
  hookName: string,
  body: () => void | Promise<void>
): Promise<void>;
