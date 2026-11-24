import { FLAG_IS_ON } from '../../../shared/flag'

const SESSION_METHODS = ['reached.constant'] as const

export const PAIRED_DESKTOP_METHODS = [
  'reached.allowed',
  ...(FLAG_IS_ON ? (['reached.flagged'] as const) : [])
] as const

export const EXECUTION_HOST_METHODS = [
  'reached.stream',
  ...SESSION_METHODS,
  'unreached.allowed',
  'unreached.capability',
  'unreached.begin'
] as const

export const MOBILE_RPC_METHOD_ROUTES: ReadonlyMap<string, string> = new Map([
  ...PAIRED_DESKTOP_METHODS.map((method) => [method, 'paired-desktop'] as const),
  ...EXECUTION_HOST_METHODS.map((method) => [method, 'execution-host'] as const)
])
