export interface ChatImageFile {
  uri: string;
  name: string;
  mimeType: string;
}

export type ImageActions = {
  copy?: () => Promise<void>;
  save?: () => Promise<void>;
  share?: () => Promise<void>;
};
