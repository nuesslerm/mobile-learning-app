import { Logger } from '@lib/logging';
import type { ImageActions } from '../domain/imageActions';
import { useCodingAgentPorts } from '../state/ports';
import { useEffect, useState } from 'react';
import { useSkin } from '@lib/skin';
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
  const { ChatImage, ChatImageViewer } = useSkin();
  const insets = useSafeAreaInsets();
  const { imageActionsFor } = useCodingAgentPorts();
  const [source, setSource] = useState<T3ImageSource>();
  const [failed, setFailed] = useState(false);
  const [viewing, setViewing] = useState(false);
  const { id, name, mimeType } = attachment;
  const [actions, setActions] = useState<ImageActions>();
  const uri = source?.uri;
  useEffect(() => {
    setActions(undefined);
    if (!viewing || !uri) return;
    const controller = new AbortController();
    Promise.resolve(imageActionsFor({uri, name, mimeType}, controller.signal)).then(
      result => { if (!controller.signal.aborted) setActions(result); },
      () => { if (!controller.signal.aborted) Logger.warn('coding_agent.chat.image_actions_unavailable'); },
    );
    return () => controller.abort();
  }, [viewing, uri, name, mimeType, imageActionsFor]);
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
          actions={actions}
          bottomInset={insets.bottom}
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
