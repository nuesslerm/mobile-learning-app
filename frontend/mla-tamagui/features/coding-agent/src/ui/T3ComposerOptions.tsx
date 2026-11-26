import { useCallback } from 'react';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import type {
  T3Conversation,
  T3RuntimeMode,
  T3Thread,
} from '../domain/t3Mobile';

const modes: readonly { slug: T3RuntimeMode; name: string }[] = [
  { slug: 'approval-required', name: 'Ask' },
  { slug: 'auto-accept-edits', name: 'Accept edits' },
  { slug: 'auto', name: 'Auto' },
  { slug: 'full-access', name: 'Full access' },
];

export function T3ComposerOptions({
  thread,
  session,
  newId,
  visible,
  onVisibleChange,
}: {
  visible: boolean;
  onVisibleChange: (visible: boolean) => void;
  thread: T3Thread;
  session: T3Conversation;
  newId: () => string;
}) {
  const { ComposerOptions } = useAgentComponents();
  const provider =
    thread.modelSelection?.instanceId ?? thread.providerInstanceId;
  const loadModels = useCallback(
    () => provider ? session.models(provider) : Promise.resolve([]),
    [session, provider]
  );
  if (!provider) return null;
  const permissions = modes.filter(
    ({ slug }) =>
      slug === thread.runtimeMode ||
      slug === 'approval-required' ||
      slug === 'full-access' ||
      (slug === 'auto-accept-edits' && provider === 'claudeAgent')
  );
  return (
    <ComposerOptions
      visible={visible}
      onVisibleChange={onVisibleChange}
      provider={provider}
      selectedModel={thread.modelSelection?.model}
      permission={thread.runtimeMode ?? undefined}
      permissions={permissions}
      loadModels={loadModels}
      setModel={(model) => session.setModel(newId(), provider, model)}
      setPermission={(value) => {
        const permission = permissions.find(({ slug }) => slug === value);
        if (!permission)
          return Promise.reject(new Error('Unknown permission mode'));
        return session.setRuntimeMode(newId(), permission.slug);
      }}
    />
  );
}
