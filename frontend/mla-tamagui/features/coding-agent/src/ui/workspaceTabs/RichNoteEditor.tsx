import { Logger } from '@lib/logging';
import { useMemo, useEffect, useRef, useState } from 'react';
import {
  Linking,
  Platform,
  ScrollView,
  TextInput,
  View,
  Text,
  Pressable,
} from 'react-native';
import {
  Pilcrow,
  Heading1,
  Heading2,
  Heading3,
  Bold,
  Italic,
  Strikethrough,
  List,
  ListOrdered,
  ListTodo,
  Quote,
  Code,
  FileCode,
  Link,
  Image,
} from 'lucide-react-native';
import { editorHtml } from './richEditorDocument';
import { useTabColors } from './TabControls';

const items = [
  ['paragraph', 'Body', Pilcrow],
  ['heading1', 'H1', Heading1],
  ['heading2', 'H2', Heading2],
  ['heading3', 'H3', Heading3],
  ['bold', 'Bold', Bold],
  ['italic', 'Italic', Italic],
  ['strike', 'Strikethrough', Strikethrough],
  ['bulletList', 'Bullet list', List],
  ['orderedList', 'Numbered list', ListOrdered],
  ['taskList', 'Task list', ListTodo],
  ['quote', 'Quote', Quote],
  ['inlineCode', 'Inline code', Code],
  ['codeBlock', 'Code block', FileCode],
  ['link', 'Link', Link],
  ['image', 'Image', Image],
] as const;
const source = { html: editorHtml };
export function RichNoteEditor({
  value,
  editable,
  onChange,
}: {
  value: string;
  editable: boolean;
  onChange: (text: string) => void;
}) {
  const c = useTabColors();
  const bridge = useRef<import('react-native-webview').WebView>(null);
  const [ready, setReady] = useState(false);
  const seed = useRef(value);
  const last = useRef<string | undefined>(undefined);
  const [prompt, setPrompt] = useState('');
  const [url, setUrl] = useState('');
  const browser = useRef<HTMLIFrameElement>(null);
  const receive = useRef((_: string) => {});
  const inject = (code: string) => {
    if (Platform.OS === 'web')
      browser.current?.contentWindow?.postMessage({ editorCommand: code }, '*');
    else bridge.current?.injectJavaScript(code + ';true');
  };
  receive.current = (raw) => {
    const m = JSON.parse(raw);
    if (m.type === 'ready') setReady(true);
    if (m.type === 'change' && m.markdown !== last.current) {
      last.current = m.markdown;
      onChange(m.markdown);
    }
    if (
      m.type === 'openLink' &&
      typeof m.url === 'string' &&
      /^https?:\/\//i.test(m.url)
    ) {
      void Linking.openURL(m.url).catch((cause) =>
        Logger.withError(cause).warn('coding_agent.note_link_failed')
      );
    }
    if (m.type === 'prompt') {
      setUrl('');
      setPrompt(m.kind);
    }
  };
  useEffect(() => {
    if (Platform.OS !== 'web') return;
    const message = (e: MessageEvent) => {
      if (
        e.source === browser.current?.contentWindow &&
        typeof e.data?.editorMessage === 'string'
      )
        receive.current(e.data.editorMessage);
    };
    window.addEventListener('message', message);
    return () => window.removeEventListener('message', message);
  }, []);
  useEffect(() => {
    if (ready) {
      last.current = seed.current;
      inject('window.note.setMarkdown(' + JSON.stringify(seed.current) + ',0)');
    }
  }, [ready]);
  useEffect(() => {
    if (ready)
      inject('window.note.setEditable(' + JSON.stringify(editable) + ')');
  }, [ready, editable]);
  const WebView =
    Platform.OS === 'web' ? null : require('react-native-webview').WebView;
  const surface = useMemo(
    () =>
      WebView ? (
        <WebView
          ref={bridge}
          source={source}
          originWhitelist={['about:blank']}
          keyboardDisplayRequiresUserAction={false}
          hideKeyboardAccessoryView
          onMessage={(e: { nativeEvent: { data: string } }) =>
            receive.current(e.nativeEvent.data)
          }
          style={{ flex: 1 }}
        />
      ) : (
        <iframe
          ref={browser}
          title="Markdown note"
          sandbox="allow-scripts"
          style={{ border: 0, flex: 1, width: '100%' }}
          srcDoc={webDocument}
        />
      ),
    [WebView]
  );
  const finish = (answer: string | null) => {
    inject('window.noteUrl(' + JSON.stringify(answer) + ')');
    setPrompt('');
  };
  return (
    <View style={{ flex: 1, backgroundColor: c.bg }}>
      <View style={{ height: 42, borderBottomWidth: 1, borderColor: c.border }}>
        <ScrollView
          horizontal
          keyboardShouldPersistTaps="always"
          showsHorizontalScrollIndicator={false}
          contentContainerStyle={{ alignItems: 'center', paddingHorizontal: 6 }}
        >
          {items.map(([cmd, label, Icon]) => (
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={label}
              disabled={!ready || !editable}
              key={cmd}
              style={{
                width: 36,
                height: 40,
                alignItems: 'center',
                justifyContent: 'center',
              }}
              onPress={() =>
                inject('window.note.runCommand(' + JSON.stringify(cmd) + ')')
              }
            >
              <Icon size={16} color={c.ink} />
            </Pressable>
          ))}
        </ScrollView>
      </View>
      {surface}
      {prompt !== '' && (
        <View
          style={{
            flexDirection: 'row',
            alignItems: 'center',
            padding: 8,
            gap: 8,
          }}
        >
          <TextInput
            accessibilityLabel={prompt + ' URL'}
            value={url}
            onChangeText={setUrl}
            placeholder="https://"
            autoFocus
            autoCapitalize="none"
            style={{ flex: 1, color: c.ink }}
          />
          <Text onPress={() => finish(null)} style={{ color: c.ink }}>
            Cancel
          </Text>
          <Text onPress={() => finish(url)} style={{ color: c.ink }}>
            Insert
          </Text>
        </View>
      )}
    </View>
  );
}
const webDocument = editorHtml.replace(
  '<script>',
  `<script>window.ReactNativeWebView={postMessage:editorMessage=>parent.postMessage({editorMessage},'*')};window.addEventListener('message',e=>{if(e.source!==parent||typeof e.data?.editorCommand!=='string')return;const s=document.createElement('script');s.textContent=e.data.editorCommand;document.body.appendChild(s);s.remove()});</script><script>`
);
