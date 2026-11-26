import { routerHarness, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../__tests__/flowApp';
import { createMobileDouble } from '../../testing/mobilePort';
import { createSettingsPortDoubles } from '../../testing/settingsPorts';



/** The staged home with the settings ports as doubles. */
const settings = (options: Parameters<typeof createSettingsPortDoubles>[0] = {}) => {
  const doubles = createSettingsPortDoubles(options);
  return {
    doubles,
    overrides: {
      'coding-agent': {
        mobile: createMobileDouble(),
        preferences: doubles.preferences,
        appUpdate: doubles.appUpdate,
        appInfo: doubles.appInfo,
        externalLinks: doubles.externalLinks,
      },
    },
  };
};

describe("The Orca home's settings tab", () => {
  beforeEach(() => {
    routerHarness().reset();
  });

  it('Given the staged home, when the user opens the settings tab, then it carries the original menu in order', async () => {
    const { overrides } = settings();
    await app.render('/coding-agent', { overrides });
    await screen.findByRole('tab', { name: 'Settings' });

    await userEvent.click(screen.getByRole('tab', { name: 'Settings' }));

    expect(
      screen.getAllByRole('button').map((button) => button.getAttribute('aria-label'))
    ).toEqual([
      'Appearance',
      'Terminal',
      'Chat UI',
      'Browser',
      'Voice',
      'Notifications',
      'Troubleshooting',
      'About',
      'Check for updates, Never',
      'Privacy Policy',
      'Support',
    ]);
  });

  it('Given the settings tab, when the user picks a screen row, then it opens the screen the original names', async () => {
    const { overrides } = settings();
    await app.render('/coding-agent', { overrides });
    await screen.findByRole('tab', { name: 'Settings' });

    await userEvent.click(screen.getByRole('tab', { name: 'Settings' }));
    await userEvent.click(screen.getByRole('button', { name: 'Terminal' }));

    expect(routerHarness().lastPush()).toBe('/coding-agent/terminal-settings');
  });

  it('Given the settings tab, when the user picks a link row, then it opens the original address', async () => {
    const { doubles, overrides } = settings();
    await app.render('/coding-agent', { overrides });
    await screen.findByRole('tab', { name: 'Settings' });

    await userEvent.click(screen.getByRole('tab', { name: 'Settings' }));
    await userEvent.click(screen.getByRole('button', { name: 'Privacy Policy' }));
    await userEvent.click(screen.getByRole('button', { name: 'Support' }));

    expect(doubles.recorded.opened).toEqual([
      'https://orca.dev/privacy',
      'https://github.com/stablyai/orca/issues',
    ]);
  });

  it('Given the settings tab, then it never claims an update that does not exist', async () => {
    const { overrides } = settings();
    await app.render('/coding-agent', { overrides });
    await screen.findByRole('tab', { name: 'Settings' });

    await userEvent.click(screen.getByRole('tab', { name: 'Settings' }));

    expect(screen.queryByText(/Update to Orca/)).toBeNull();
    expect(screen.getByText('Version 1.0.0')).toBeTruthy();
  });

  it('Given the settings tab, when the user asks for a check, then the check runs and the row reports it', async () => {
    const { doubles, overrides } = settings();
    await app.render('/coding-agent', { overrides });
    await screen.findByRole('tab', { name: 'Settings' });

    await userEvent.click(screen.getByRole('tab', { name: 'Settings' }));
    expect(doubles.recorded.checks).toBe(0);

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates, Never' }));

    await waitFor(() => expect(doubles.recorded.checks).toBe(1));
    expect(
      await screen.findByRole('button', { name: 'Check for updates, Up to date' })
    ).toBeTruthy();
  });

  it('Given the settings tab, when a check finds an update, then the row offers it', async () => {
    const { doubles, overrides } = settings({
      update: { status: 'available', updateId: 'update-7' },
    });
    await app.render('/coding-agent', { overrides });
    await screen.findByRole('tab', { name: 'Settings' });

    await userEvent.click(screen.getByRole('tab', { name: 'Settings' }));
    await userEvent.click(screen.getByRole('button', { name: 'Check for updates, Never' }));

    await userEvent.click(await screen.findByRole('button', { name: 'Update available' }));

    expect(doubles.recorded.installed).toEqual(['update-7']);
  });
});

describe('The settings screens', () => {
  beforeEach(() => {
    routerHarness().reset();
  });

  it('Given the appearance screen, when the user picks a theme, then the device keeps it', async () => {
    const { doubles, overrides } = settings();
    await app.render('/coding-agent/appearance-settings', { overrides });

    await userEvent.click(screen.getByRole('button', { name: 'Dark' }));

    expect(doubles.recorded.appearance).toBe('dark');
  });

  it('Given the notifications screen, when the user opts in, then the device keeps it', async () => {
    const { doubles, overrides } = settings();
    await app.render('/coding-agent/notifications', { overrides });

    await userEvent.click(screen.getByRole('switch'));

    expect(doubles.recorded.notificationsEnabled).toBe(true);
  });

  it('Given the terminal screen, when it opens, then it names what it cannot offer', async () => {
    const { overrides } = settings();
    await app.render('/coding-agent/terminal-settings', { overrides });

    expect(screen.getByText('Terminal')).toBeTruthy();
    expect(screen.getByText('Restore the terminal when you leave the app')).toBeTruthy();
  });
});
