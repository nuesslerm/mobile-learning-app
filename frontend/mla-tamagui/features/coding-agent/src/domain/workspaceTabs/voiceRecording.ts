// T3 accepts a length-prefixed options object followed by mono 16 kHz Float32 samples.
export function voiceRecording(wav: ArrayBuffer): ArrayBuffer {
  const view = new DataView(wav);
  const tag = (offset: number) =>
    String.fromCharCode(...new Uint8Array(wav, offset, 4));
  if (wav.byteLength < 44 || tag(0) !== 'RIFF' || tag(8) !== 'WAVE')
    throw new Error('The microphone did not produce a WAV recording.');
  let hz = 0,
    channels = 0;
  for (let pos = 12; pos + 8 <= wav.byteLength; ) {
    const size = view.getUint32(pos + 4, true),
      start = pos + 8;
    if (start + size > wav.byteLength) break;
    if (tag(pos) === 'fmt ' && size >= 16) {
      if (
        view.getUint16(start, true) !== 1 ||
        view.getUint16(start + 14, true) !== 16
      )
        break;
      channels = view.getUint16(start + 2, true);
      hz = view.getUint32(start + 4, true);
    }
    if (tag(pos) === 'data' && hz > 0 && channels > 0) {
      const count = Math.floor(((size / (2 * channels)) * 16000) / hz);
      if (!count) break;
      const options = new TextEncoder().encode(
        JSON.stringify({
          speechLanguage:
            Intl.DateTimeFormat().resolvedOptions().locale.split('-')[0] ||
            'en',
          speechCustomWords: [],
          speechTranslateToEnglish: false,
          speechRemoveFillerWords: true,
        })
      );
      const body = new ArrayBuffer(4 + options.length + count * 4),
        out = new DataView(body);
      out.setUint32(0, options.length, true);
      new Uint8Array(body, 4, options.length).set(options);
      for (let i = 0; i < count; i++)
        out.setFloat32(
          4 + options.length + i * 4,
          view.getInt16(
            start + Math.floor((i * hz) / 16000) * 2 * channels,
            true
          ) / 32768,
          true
        );
      return body;
    }
    pos = start + size + (size % 2);
  }
  throw new Error('Record a little longer and try again.');
}
