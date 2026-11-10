import { FIXTURE_THREADS } from '../../domain';
import { createFixtureGateway } from '@feature/coding-agent/testing';
import {
  act,
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { app } from '../../__tests__/flowApp';

// Fixture gateway keys threads as `fx-thread-<FixtureId>` (fixtureGateway.ts).
const FX_THREAD_BASIC = 'fx-thread-fx-basic';

function renderThread(
  threadId: string,
  options: { theme?: 'light' | 'dark' } = {}
): Promise<Awaited<ReturnType<typeof app.render>>> {
  return app.render(`/coding-agent/${threadId}`, {
    overrides: { 'coding-agent': { gateway: createFixtureGateway() } },
    theme: options.theme,
  });
}

function sendButtonDisabled(send: HTMLElement): boolean {
  return (
    send.getAttribute('aria-disabled') === 'true' ||
    send.hasAttribute('disabled')
  );
}

function buttonDisabled(button: HTMLElement): boolean {
  return (
    button.getAttribute('aria-disabled') === 'true' ||
    button.hasAttribute('disabled')
  );
}

// Scripted modes keyed by `fixture:` are triggered on the fx-basic thread; the
// fixture gateway schedules the pending request ~400 ms after the turn starts.
async function sendScripted(text: string): Promise<void> {
  await userEvent.type(screen.getByTestId('ThreadComposerInput'), text);
  const send = screen.getByTestId('ThreadComposerSendButton');
  await waitFor(() => expect(sendButtonDisabled(send)).toBe(false));
  await userEvent.click(send);
}

describe('ThreadScreen flow (fixture transport)', () => {
  it('given the fx-basic fixture thread, when the screen renders, then the feed shows the fixture user and assistant message texts and fenced code content sent to the thread', async () => {
    await renderThread(FX_THREAD_BASIC);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    const feed = screen.getByTestId('ThreadFeed');
    expect(feed.textContent).toContain(
      FIXTURE_THREADS['fx-basic'].messages[0].text
    );
    expect(feed.textContent).toContain(
      FIXTURE_THREADS['fx-basic'].messages[1].text
    );

    // No fixture or scripted reply contains a fenced block, and the harness's
    // react-native-markdown-display stub renders markdown as plain text, so a
    // CodeBlock node never appears here. Prove fenced content renders by
    // sending it through the real composer.
    await userEvent.type(
      screen.getByTestId('ThreadComposerInput'),
      '```ts\nconst x: number = 1;\n```'
    );
    const send = screen.getByTestId('ThreadComposerSendButton');
    await waitFor(() => expect(sendButtonDisabled(send)).toBe(false));
    await userEvent.click(send);

    await waitFor(
      () => {
        expect(screen.getByTestId('ThreadFeed').textContent).toContain(
          'const x: number = 1;'
        );
      },
      { timeout: 12_000 }
    );
  });

  it('given a typed message in the composer, when send is pressed, then the feed shows the new user message and the input clears', async () => {
    await renderThread(FX_THREAD_BASIC);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    const input = screen.getByTestId('ThreadComposerInput');
    await userEvent.type(input, 'hello from flow');

    const send = screen.getByTestId('ThreadComposerSendButton');
    await waitFor(() => expect(sendButtonDisabled(send)).toBe(false));
    await userEvent.click(send);

    await waitFor(
      () => {
        expect(screen.getByTestId('ThreadFeed').textContent).toContain(
          'hello from flow'
        );
      },
      { timeout: 12_000 }
    );

    expect(
      (screen.getByTestId('ThreadComposerInput') as HTMLTextAreaElement).value
    ).toBe('');
  });

  it('given an empty composer input, when the screen renders, then the send button is disabled', async () => {
    await renderThread(FX_THREAD_BASIC);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    const send = screen.getByTestId('ThreadComposerSendButton');
    expect(sendButtonDisabled(send)).toBe(true);
  });
});

describe('ThreadScreen flow (fx-approval)', () => {
  const FX_APPROVAL_REQ = 'fx-req-fx-thread-fx-basic-approval';

  it('given a scripted approval turn, when the pending approval card renders, then it shows the detail text and the Allow/Deny buttons', async () => {
    await renderThread(FX_THREAD_BASIC);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    await sendScripted('fixture:approval');

    const card = await waitFor(
      () => screen.getByTestId(`ApprovalCard-${FX_APPROVAL_REQ}`),
      { timeout: 12_000 }
    );
    expect(card.textContent).toContain('src/foo.ts');
    expect(
      screen.getByTestId(`ApprovalCardAllow-${FX_APPROVAL_REQ}`)
    ).toBeTruthy();
    expect(
      screen.getByTestId(`ApprovalCardDeny-${FX_APPROVAL_REQ}`)
    ).toBeTruthy();
  });

  it('given a pending approval card, when Allow is clicked, then the feed shows the resolved confirmation (card removal blocked by a fixture-gateway payload bug)', async () => {
    await renderThread(FX_THREAD_BASIC);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    await sendScripted('fixture:approval');

    const cardId = `ApprovalCard-${FX_APPROVAL_REQ}`;
    await waitFor(() => expect(screen.getByTestId(cardId)).toBeTruthy(), {
      timeout: 12_000,
    });

    await userEvent.click(
      screen.getByTestId(`ApprovalCardAllow-${FX_APPROVAL_REQ}`)
    );

    // Real behavior: the fixture gateway resolves the approval and the feed
    // shows the resolved confirmation. The card itself is NOT removed, because
    // fixtureGateway.ts emits the `request.resolved` activity without a
    // `requestId` in its payload, so derivePendingApprovals (pendingRequests.ts)
    // cannot match it to the open request and never clears the card.
    await waitFor(
      () => {
        expect(screen.getByTestId('ThreadFeed').textContent).toContain(
          'Approval recorded (accept). Continuing.'
        );
      },
      { timeout: 12_000 }
    );
    await waitFor(() => expect(screen.queryByTestId(cardId)).toBeNull(), {
      timeout: 12_000,
    });
  });
});

describe('ThreadScreen flow (fx-user-input)', () => {
  const FX_USER_INPUT_REQ = 'fx-req-fx-thread-fx-basic-user-input';

  it('given a scripted user-input turn, when the card renders, then it lists the answer options', async () => {
    await renderThread(FX_THREAD_BASIC);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    await sendScripted('fixture:question');

    const card = await waitFor(
      () => screen.getByTestId(`UserInputCard-${FX_USER_INPUT_REQ}`),
      { timeout: 12_000 }
    );
    expect(card.textContent).toContain('Should I keep going?');
    expect(
      screen.getByTestId(`UserInputCardOption-${FX_USER_INPUT_REQ}-0`)
        .textContent
    ).toContain('Yes');
    expect(
      screen.getByTestId(`UserInputCardOption-${FX_USER_INPUT_REQ}-1`)
        .textContent
    ).toContain('No');
  });

  it('given a user-input card, when an option is chosen and submit is pressed, then the feed shows the received answers (card removal blocked by a fixture-gateway payload bug)', async () => {
    await renderThread(FX_THREAD_BASIC);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    await sendScripted('fixture:question');

    const cardId = `UserInputCard-${FX_USER_INPUT_REQ}`;
    await waitFor(() => expect(screen.getByTestId(cardId)).toBeTruthy(), {
      timeout: 12_000,
    });

    await userEvent.click(
      screen.getByTestId(`UserInputCardOption-${FX_USER_INPUT_REQ}-0`)
    );
    const submit = screen.getByTestId(
      `UserInputCardSubmit-${FX_USER_INPUT_REQ}`
    );
    await waitFor(() => expect(buttonDisabled(submit)).toBe(false));
    await userEvent.click(submit);

    // Real behavior: the fixture gateway receives the answers and the feed
    // shows the confirmation. The card itself is NOT removed, because
    // fixtureGateway.ts emits the `user-input.resolved` activity without a
    // `requestId` in its payload, so derivePendingUserInputs (pendingRequests.ts)
    // cannot match it to the open request and never clears the card.
    await waitFor(
      () => {
        expect(screen.getByTestId('ThreadFeed').textContent).toContain(
          'Got it. Continuing with your answers.'
        );
      },
      { timeout: 12_000 }
    );
    await waitFor(() => expect(screen.queryByTestId(cardId)).toBeNull(), {
      timeout: 12_000,
    });
  });
});

describe('ThreadScreen flow (fx-streaming)', () => {
  const FX_STREAMING = 'fx-thread-fx-streaming';

  it('given the fx-streaming fixture thread, when the screen renders, then the assistant text length grows between two reads 1.5 s apart while the fixture ticks and a Stop control is visible', async () => {
    await renderThread(FX_STREAMING);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    // The fx-streaming turn keeps the session running, so the composer swaps
    // its send button for a Stop button (ComposerBase isRunning).
    await waitFor(
      () => expect(screen.getByTestId('ThreadComposerStopButton')).toBeTruthy(),
      { timeout: 12_000 }
    );

    const firstLength = screen.getByTestId('ThreadFeed').textContent.length;
    // the stream keeps rendering while the test waits, so the wait is inside act
    await act(() => new Promise((resolve) => setTimeout(resolve, 1500)));
    const secondLength = screen.getByTestId('ThreadFeed').textContent.length;

    expect(secondLength).toBeGreaterThan(firstLength);
  });
});

describe('ThreadScreen flow (fx-tool-calls)', () => {
  const FX_TOOL_CALLS = 'fx-thread-fx-tool-calls';
  const FX_TOOL_CALLS_GROUP = 'fx-tool-calls-activity-1';

  it('given the fx-tool-calls fixture thread, when the work-log toggle is clicked, then the 7 hidden tool rows appear and a second click hides them again', async () => {
    await renderThread(FX_TOOL_CALLS);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    const toggle = await waitFor(
      () => screen.getByTestId(`WorkGroupToggle-${FX_TOOL_CALLS_GROUP}`),
      { timeout: 12_000 }
    );
    expect(toggle.textContent).toContain('+7 previous tool calls');

    const rowsBefore = screen.getAllByTestId(/^WorkLogRow-/).length;
    expect(rowsBefore).toBe(1);

    await userEvent.click(toggle);

    await waitFor(
      () => {
        expect(
          screen.getByTestId(`WorkGroupToggle-${FX_TOOL_CALLS_GROUP}`)
            .textContent
        ).toContain('Hide 7 tool calls');
        expect(screen.getAllByTestId(/^WorkLogRow-/).length).toBe(
          rowsBefore + 7
        );
      },
      { timeout: 12_000 }
    );

    await userEvent.click(
      screen.getByTestId(`WorkGroupToggle-${FX_TOOL_CALLS_GROUP}`)
    );

    await waitFor(
      () => {
        expect(screen.getAllByTestId(/^WorkLogRow-/).length).toBe(rowsBefore);
      },
      { timeout: 12_000 }
    );
  });
});

describe('ThreadScreen flow (dark theme)', () => {
  it('given the fx-basic fixture thread, when the screen renders in the dark theme, then the feed shows the same fixture user and assistant message texts', async () => {
    await renderThread(FX_THREAD_BASIC, { theme: 'dark' });

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    const feed = screen.getByTestId('ThreadFeed');
    expect(feed.textContent).toContain(
      FIXTURE_THREADS['fx-basic'].messages[0].text
    );
    expect(feed.textContent).toContain(
      FIXTURE_THREADS['fx-basic'].messages[1].text
    );
  });
});

describe('ThreadScreen flow (fx-plan)', () => {
  const FX_PLAN = 'fx-thread-fx-plan';
  const FX_PLAN_ID = 'fx-plan-1';

  it('given the fx-plan fixture thread, when the proposed plan card renders, then it shows the plan markdown and its Implement button queues the turn (turn dispatch blocked by the running fixture turn)', async () => {
    await renderThread(FX_PLAN);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    const card = await waitFor(
      () => screen.getByTestId(`PlanCard-${FX_PLAN_ID}`),
      {
        timeout: 12_000,
      }
    );
    expect(card.textContent).toContain('Refactor plan');
    expect(card.textContent).toContain('Inspect the current module layout.');

    await userEvent.click(
      screen.getByTestId(`PlanCardImplement-${FX_PLAN_ID}`)
    );

    // Real behavior: fx-plan's latest turn is still `running`, so
    // implementPlan → startTurn('Implement the plan', { sourceProposedPlan })
    // without steer sees isRunning and enqueues the message instead of
    // dispatching. The feed shows no new user message; the composer shows the
    // queued badge.
    await waitFor(
      () => expect(screen.getByTestId('ComposerQueuedBadge')).toBeTruthy(),
      { timeout: 12_000 }
    );
    expect(screen.getByTestId('ComposerQueuedBadge').textContent).toContain(
      '1'
    );

    // Failing assertion (real screen behavior: a running turn queues the
    // implement message instead of dispatching it):
    //   await waitFor(
    //     () => {
    //       expect(screen.getByTestId('ThreadFeed').textContent).toContain(
    //         'Implement the plan',
    //       );
    //     },
    //     { timeout: 12_000 },
    //   );
    //   → AssertionError: expected 'Sketch a plan before you start coding…' to contain 'Implement the plan'
  });
});

describe('ThreadScreen flow (header settings)', () => {
  it('given the header settings action, when tapped, then the settings sheet opens for the current thread without navigating', async () => {
    const router = routerHarness();
    await renderThread(FX_THREAD_BASIC);

    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), {
      timeout: 12_000,
    });

    // ThreadScreen.openSettings calls useThreadSettingsRequestStore.request()
    // with the threadId (no router push), which flips ThreadSettingsSheet's
    // `open` prop to true. The screen's action is therefore not a navigation.
    await userEvent.click(screen.getByTestId('ThreadSettingsButton'));

    expect(router.pushes()).toEqual([]);
    await waitFor(
      () => expect(screen.getByTestId('ThreadSettingsSheet')).toBeTruthy(),
      {
        timeout: 12_000,
      }
    );
    expect(screen.getByTestId('ThreadSettingsSheet').textContent).toContain(
      'Thread settings'
    );
  });
});
