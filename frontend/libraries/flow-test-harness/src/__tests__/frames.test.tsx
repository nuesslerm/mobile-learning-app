import { waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

describe('given an iframe with a srcdoc page that posts to its parent', () => {
  it('when it is attached, then its script runs and the message names the frame as its source', async () => {
    const received: MessageEvent[] = [];
    const onMessage = (event: MessageEvent) => received.push(event);
    window.addEventListener('message', onMessage);

    const frame = document.createElement('iframe');
    frame.setAttribute('srcdoc', "<body><p id='x'>hi</p><script>window.parent.postMessage('ready', '*')</script></body>");
    document.body.appendChild(frame);

    await waitFor(() => expect(received).toHaveLength(1));
    expect(received[0]?.data).toBe('ready');
    expect(received[0]?.source).toBe(frame.contentWindow);
    expect(frame.contentDocument?.getElementById('x')?.textContent).toBe('hi');

    window.removeEventListener('message', onMessage);
    frame.remove();
  });

  it('when the page measures a Range, then it gets empty rects instead of a crash', async () => {
    const frame = document.createElement('iframe');
    frame.setAttribute('srcdoc', '<body><p>t</p></body>');
    document.body.appendChild(frame);
    await waitFor(() => expect(frame.contentDocument?.querySelector('p')).toBeTruthy());

    const range = frame.contentDocument!.createRange();
    expect(Array.from(range.getClientRects())).toEqual([]);
    expect(range.getBoundingClientRect().width).toBe(0);
    frame.remove();
  });
});
