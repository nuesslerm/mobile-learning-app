import { useEffect, useState } from 'react';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import { Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type {
  T3Attachment,
  T3Conversation,
  T3ImageSource,
} from '../domain/t3Mobile';

function ImageAttachment({
  attachment,
  session,
}: {
  attachment: T3Attachment;
  session?: T3Conversation;
}) {
  const { ChatImage, ChatImageViewer } = useAgentComponents();
  const insets = useSafeAreaInsets();
  const [source, setSource] = useState<T3ImageSource>();
  const [failed, setFailed] = useState(false);
  const [viewing, setViewing] = useState(false);
  const { id, name, mimeType } = attachment;
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    setFailed(false);
    setSource(undefined);
    if (!session) return;
    const load = () => {
      session.resolveImage({ id, name, mimeType, type: 'image' }).then(
        (result) => {
          if (!active) return;
          setSource(result);
          timer = setTimeout(
            load,
            Math.max(1000, result.expiresAt - Date.now() - 30000)
          );
        },
        () => {
          if (active) setFailed(true);
        }
      );
    };
    load();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [session, id, name, mimeType]);
  return (
    <>
      <ChatImage
        label={name}
        uri={source?.uri}
        unavailable={failed}
        onOpen={() => setViewing(true)}
      />
      {source && viewing ? (
        <ChatImageViewer
          label={name}
          uri={source.uri}
          topInset={insets.top}
          rightInset={insets.right}
          onClose={() => setViewing(false)}
        />
      ) : null}
    </>
  );
}

export function T3ChatAttachments({
  attachments,
  session,
}: {
  attachments: readonly T3Attachment[];
  session?: T3Conversation;
}) {
  return (
    <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
      {attachments.map((attachment) =>
        attachment.type === 'image' ? (
          <ImageAttachment
            key={attachment.id}
            attachment={attachment}
            session={session}
          />
        ) : (
          <Text key={attachment.id} selectable>
            {attachment.name}
          </Text>
        )
      )}
    </View>
  );
}
