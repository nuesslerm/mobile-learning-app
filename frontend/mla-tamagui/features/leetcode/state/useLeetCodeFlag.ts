import type { LeetCodeFlags } from '../domain/featureFlags';
import { useLeetCodePorts } from './ports';

export function useLeetCodeFlag(name: keyof LeetCodeFlags): boolean {
  return useLeetCodePorts().flags[name];
}
