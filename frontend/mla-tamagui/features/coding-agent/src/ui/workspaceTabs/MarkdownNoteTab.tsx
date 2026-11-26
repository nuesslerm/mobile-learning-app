import { useEffect, useRef, useState } from 'react';
import { Button, Text, TextInput, View } from 'react-native';
import type { Pairing } from '../../domain/mobile';
import type {
  NoteDraft,
  WorkspaceTabsPort,
} from '../../domain/workspaceTabs/tabs';
import { Logger } from '@lib/logging';

type Props = {
  api: WorkspaceTabsPort;
  pair: Pairing;
  checkout: string;
  path: string;
  color: string;
};
export function MarkdownNoteTab({ api, pair, checkout, path, color }: Props) {
  const [draft, setDraft] = useState<NoteDraft>();
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const writes = useRef(Promise.resolve());
  const [revision, refresh] = useState(0);
  useEffect(() => {
    let live = true;
    void Promise.all([
      api.read(pair, checkout, path),
      api.draft(pair, checkout, path),
    ])
      .then(([file, pending]) => {
        if (!live) return;
        setDraft(pending ?? { previous: file.content, content: file.content });
        setError(
          pending && pending.previous !== file.content
            ? 'The desktop file changed. Your draft is saved on this device.'
            : ''
        );
      })
      .catch((cause) => {
        if (live) setError(cause.message);
        Logger.withError(cause).warn('coding_agent.note_read_failed');
      });
    return () => {
      live = false;
    };
  }, [api, pair, checkout, path, revision]);
  const persist = (next: NoteDraft | null) => {
    writes.current = writes.current
      .then(() => api.keepDraft(pair, checkout, path, next))
      .catch((cause) => {
        Logger.withError(cause).warn('coding_agent.note_draft_failed');
        setError('Could not store the draft on this device.');
      });
    return writes.current;
  };
  async function save() {
    if (!draft || saving) return;
    setSaving(true);
    try {
      const file = await api.save(pair, checkout, path, draft);
      setDraft({ previous: file.content, content: file.content });
      await persist(null);
      setError('');
    } catch (cause) {
      Logger.withError(cause).warn('coding_agent.note_save_failed');
      setError(cause instanceof Error ? cause.message : 'Could not save note.');
    } finally {
      setSaving(false);
    }
  }
  return (
    <View style={{ flex: 1, padding: 12 }}>
      {draft && (
        <TextInput
          accessibilityLabel="Markdown note"
          multiline
          editable={!saving}
          textAlignVertical="top"
          style={{ flex: 1, color, fontSize: 16 }}
          value={draft.content}
          onChangeText={(content) => {
            const next = { ...draft, content };
            setDraft(next);
            void persist(next);
          }}
        />
      )}
      {error ? (
        <Text accessibilityRole="alert" style={{ color }}>
          {error}
        </Text>
      ) : null}
      {draft && draft.previous !== draft.content ? (
        <View style={{ flexDirection: 'row', justifyContent: 'flex-end' }}>
          <Button
            title="Discard"
            disabled={saving}
            onPress={() => {
              void persist(null).then(() => refresh((n) => n + 1));
            }}
          />
          <Button title="Save" disabled={saving} onPress={() => void save()} />
        </View>
      ) : (
        !!error && <Button title="Retry" onPress={() => refresh((n) => n + 1)} />
      )}
    </View>
  );
}
