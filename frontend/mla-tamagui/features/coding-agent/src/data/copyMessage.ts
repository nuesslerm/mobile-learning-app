import { Logger } from '@lib/logging';

export async function copyMessage(text: string): Promise<boolean> {
  try {
    const { setStringAsync } = await import('expo-clipboard');
    return await setStringAsync(text);
  } catch {
    Logger.warn('coding_agent.chat.copy_failed');
    return false;
  }
}
