export type LogLayerLevel =
  | 'trace'
  | 'debug'
  | 'info'
  | 'warn'
  | 'error'
  | 'fatal';

export function toStoreLevel(level: LogLayerLevel): Exclude<LogLayerLevel, 'trace' | 'fatal'> {
  switch (level) {
    case 'trace':
      return 'debug';
    case 'fatal':
      return 'error';
    case 'debug':
    case 'info':
    case 'warn':
    case 'error':
      return level;
  }
}
