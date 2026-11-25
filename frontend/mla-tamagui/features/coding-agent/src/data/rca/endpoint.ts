export function endpoint(input: string) {
  const url = new URL(input.trim());
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  if ((url.protocol !== 'https:' && !(local && url.protocol === 'http:')) ||
      url.username || url.password || url.search || url.hash) throw new Error('Enter an HTTPS address or a local simulator address.');
  return url.href.replace(/\/$/, '');
}
