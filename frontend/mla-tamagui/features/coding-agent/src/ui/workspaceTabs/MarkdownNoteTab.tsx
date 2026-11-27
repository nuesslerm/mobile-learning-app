import { useSkin } from '@lib/skin';
import { useEffect, useRef, useState } from 'react';
import { Button, Keyboard, Pressable, Text, View } from 'react-native';
import type { Pairing } from '../../domain/mobile';
import type {
  NoteDraft,
  WorkspaceTabsPort,
} from '../../domain/workspaceTabs/tabs';
import { Logger } from '@lib/logging';

import { RichNoteEditor } from './RichNoteEditor';
import { useTabColors } from './TabControls';
import { useCodingAgentPorts } from '../../state/ports';

type Props = {
  directory: string;
  api: WorkspaceTabsPort;
  pair: Pairing;
  checkout: string;
  path: string;
  color: string;
};
export function MarkdownNoteTab({
  api,
  pair,
  checkout,
  path,
  color,
  directory,
}: Props) {
  const { copyMessage } = useCodingAgentPorts();
  const { BottomSheet } = useSkin();
  const c = useTabColors();
  const [confirmDiscard, setConfirmDiscard] = useState(false);
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
    <View style={{ flex: 1 }}>
      {draft && (
        <RichNoteEditor
          key={revision}
          value={draft.content}
          editable={!saving}
          onChange={(content) => {
            const next = { ...draft, content };
            setDraft(next);
            void persist(next);
          }}
        />
      )}
      {draft && (
        <View
          style={{
            position: 'absolute',
            bottom: 12,
            right: 12,
            left: 12,
            alignItems: 'flex-end',
            gap: 4,
          }}
          pointerEvents="box-none"
        >
          {!!error && (
            <Text
              accessibilityRole="alert"
              style={{ color, backgroundColor: c.bg }}
            >
              {error}
            </Text>
          )}
          <View
            style={{
              flexDirection: 'row',
              flexWrap: 'wrap',
              justifyContent: 'flex-end',
              gap: 4,
            }}
          >
            {[
              {
                label: 'Copy',
                accessibility: 'Copy Markdown source',
                act: () => void copyMessage(draft.content),
              },
              {
                label: 'Copy path',
                accessibility: 'Copy Markdown path',
                act: () =>
                  void copyMessage(
                    path.startsWith('/')
                      ? path
                      : directory.replace(/\/$/, '') + '/' + path
                  ),
              },
              ...(draft.previous !== draft.content
                ? [
                    {
                      label: 'Discard',
                      accessibility: 'Discard',
                      act: () => {
                        Keyboard.dismiss();
                        setConfirmDiscard(true);
                      },
                    },
                    {
                      label: 'Save',
                      accessibility: 'Save',
                      act: () => void save(),
                    },
                  ]
                : []),
            ].map((item) => (
              <Pressable
                key={item.label}
                accessibilityRole="button"
                accessibilityLabel={item.accessibility}
                disabled={saving}
                style={{
                  backgroundColor: c.surface,
                  borderRadius: 12,
                  minHeight: 34,
                  paddingHorizontal: 12,
                  justifyContent: 'center',
                  opacity: saving ? 0.45 : 1,
                }}
                onPress={item.act}
              >
                <Text style={{ fontSize: 13, fontWeight: '600', color: c.ink }}>
                  {item.label}
                </Text>
              </Pressable>
            ))}
          </View>
        </View>
      )}
      {!draft && !!error && (
        <>
          <Text accessibilityRole="alert" style={{ color }}>
            {error}
          </Text>
          <Button title="Retry" onPress={() => refresh((n) => n + 1)} />
        </>
      )}
      {confirmDiscard && (
        <BottomSheet
          onClose={() => setConfirmDiscard(false)}
          style={{
            backgroundColor: c.bg,
            borderTopLeftRadius: 22,
            borderTopRightRadius: 22,
            padding: 16,
            paddingBottom: 32,
          }}
        >
          {(dismiss) => (
            <>
              <Text
                accessibilityRole="header"
                style={{ fontSize: 16, fontWeight: '600', color }}
              >
                Discard changes?
              </Text>
              <Text style={{ color: c.muted, marginVertical: 8 }}>
                Replace the phone draft with the latest desktop file?
              </Text>
              <View
                style={{ flexDirection: 'row', justifyContent: 'flex-end' }}
              >
                <Button title="Cancel" onPress={() => dismiss()} />
                <Button
                  title="Discard"
                  color={c.danger}
                  onPress={() =>
                    dismiss(() => {
                      setConfirmDiscard(false);
                      setDraft(undefined);
                      void persist(null).then(() => refresh((n) => n + 1));
                    })
                  }
                />
              </View>
            </>
          )}
        </BottomSheet>
      )}
    </View>
  );
}
