import type { ChatImageFile, ImageActions } from '../domain/imageActions';

export async function imageActionsFor(image: ChatImageFile, signal: AbortSignal): Promise<ImageActions> {
  if (typeof window === 'undefined') return {};
  const { name, mimeType } = image;
  let prepared: File | undefined;
  async function readFile() {
    if (prepared) return prepared;
    const response = await fetch(image.uri, {signal});
    if (!response.ok) throw new Error('Image download failed.');
    return new File([await response.blob()], name, { type: mimeType });
  }
  const actions: ImageActions = {
    async save() {
      const url = URL.createObjectURL(await readFile());
      const link = document.createElement('a');
      link.href = url;
      link.download = name;
      link.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    },
  };
  if (
    navigator.clipboard?.write &&
    typeof ClipboardItem !== 'undefined' &&
    (ClipboardItem.supports?.(mimeType) ?? mimeType === 'image/png')
  ) {
    actions.copy = () =>
      navigator.clipboard.write([
        new ClipboardItem({ [mimeType]: readFile() }),
      ]);
  }
  if (
    navigator.canShare?.({ files: [new File([], name, { type: mimeType })] })
  ) {
    // Web Share needs a user gesture, so finish downloading before showing Share.
    prepared = await readFile();
    const file = prepared;
    actions.share = async () => {
      try {
        await navigator.share({ files: [file] });
      } catch (error) {
        if (!(error instanceof DOMException && error.name === 'AbortError'))
          throw error;
      }
    };
  }
  return actions;
}
