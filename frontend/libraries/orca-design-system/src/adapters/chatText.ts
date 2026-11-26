import type { ChatTextPreference } from '@lib/agent-ui-contracts';
import { createContext, useContext } from 'react';

export const ChatTextContext = createContext<{
  value: ChatTextPreference;
  defaultSize: number;
  stored: boolean;
  update: (change: Partial<ChatTextPreference>) => void;
  reset: () => void;
}>({
  value: { size: 14, font: 'system', lineSpacing: 'default' },
  defaultSize: 14,
  stored: false,
  update: () => undefined,
  reset: () => undefined,
});

export const useChatText = () => useContext(ChatTextContext);
