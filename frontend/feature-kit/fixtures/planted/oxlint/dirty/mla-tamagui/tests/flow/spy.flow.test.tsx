import { vi } from 'vitest';
export const spy = vi.spyOn({ y: () => 1 }, 'y');
