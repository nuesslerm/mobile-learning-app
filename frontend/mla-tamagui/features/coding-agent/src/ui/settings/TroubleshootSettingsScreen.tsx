// What the coding agent knows about its own connection, as the original's
// Troubleshooting screen offers it.
//
// Everything here is read from the app's own state: the transport it uses, the
// connection it holds, the server it talks to and whether it holds a token.

import {
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
  useSkin,
} from '@lib/skin';
import { appConfig } from '@lib/app-config';
import { Logger } from '@lib/logging';
import { useEffect, useState } from 'react';

import { toWsUrl, wsUrlHost } from '../../domain';
import { useCodingAgentPorts } from '../../state/ports';
import { useGatewayStore } from '../../state/gatewayStore';
import { OrcaSettingsScreen } from './OrcaSettingsScreen';

function serverHost(): string {
  const explicit = (appConfig as { codingAgentWsUrl?: string }).codingAgentWsUrl;
  const base = appConfig.agentBaseUrl;
  if (explicit === undefined && base === undefined) {
    return 'Not configured';
  }
  try {
    return wsUrlHost(explicit ?? toWsUrl(base as string));
  } catch (cause) {
    Logger.withError(cause).warn('coding-agent.settings.server_host_unresolved');
    return 'Not configured';
  }
}

export function TroubleshootSettingsScreen() {
  const { HomeSettingsGroup, HomeSettingsRow } = useSkin();
  const { tokenStore } = useCodingAgentPorts();
  const status = useGatewayStore((state) => state.status);
  const transport = useGatewayStore((state) => state.transport);
  const [tokenStored, setTokenStored] = useState<boolean | null>(null);

  useEffect(() => {
    let active = true;
    void tokenStore.read().then((token) => {
      if (active) {
        setTokenStored(token !== null);
      }
    });
    return () => {
      active = false;
    };
  }, [tokenStore]);

  return (
    <OrcaSettingsScreen title="Troubleshooting">
      <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET_PLAIN}>
        <HomeSettingsRow label="Connection" value={status} />
        <HomeSettingsRow label="Transport" value={transport ?? 'Not connected'} />
        <HomeSettingsRow label="Server" value={serverHost()} />
        <HomeSettingsRow
          label="Access token"
          value={
            tokenStored === null
              ? 'Reading…'
              : tokenStored
                ? 'Stored'
                : 'Not stored'
          }
        />
      </HomeSettingsGroup>
    </OrcaSettingsScreen>
  );
}
