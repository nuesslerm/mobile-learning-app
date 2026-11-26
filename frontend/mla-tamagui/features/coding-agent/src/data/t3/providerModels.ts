import { z } from 'zod';
import type { T3Model, T3Thread } from '../../domain/t3Mobile';

const effortIds = new Set(['reasoningEffort', 'effort', 'reasoning', 'variant', 'reasoning_effort', 'thinking_level', 'thinking']);
const select = z.object({
  type: z.literal('select'), id: z.string(), role: z.string().optional(),
  currentValue: z.string().optional(),
  options: z.array(z.object({ id: z.string(), label: z.string(), isDefault: z.boolean().optional() })),
});
const config = z.object({ providers: z.array(z.object({
  instanceId: z.string(),
  models: z.array(z.object({
    slug: z.string(), name: z.string(),
    capabilities: z.object({ optionDescriptors: z.array(z.unknown()).optional() }).nullish(),
  })),
})) });

export function providerModels(raw: unknown, instance: string, selected: T3Thread['modelSelection']): T3Model[] {
  const provider = config.parse(raw).providers.find(row => row.instanceId === instance);
  return (provider?.models ?? []).map(({ slug, name, capabilities }) => {
    for (const item of capabilities?.optionDescriptors ?? []) {
      const parsed = select.safeParse(item);
      if (!parsed.success) continue;
      const descriptor = parsed.data;
      if (!descriptor.options.length || (descriptor.role !== 'reasoning' && !effortIds.has(descriptor.id))) continue;
      const saved = selected?.instanceId === instance && selected.model === slug ? selected.options : undefined;
      const value = Array.isArray(saved) ? saved.find(row => row.id === descriptor.id)?.value : saved?.[descriptor.id];
      return { slug, name, effort: {
        id: descriptor.id,
        choices: descriptor.options.map(option => ({ slug: option.id, name: option.label })),
        current: typeof value === 'string' ? value : descriptor.currentValue ?? descriptor.options.find(option => option.isDefault)?.id,
      } };
    }
    return { slug, name };
  });
}
