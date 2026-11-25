// features/coding-agent/src/data/externalLinks.ts
//
// The links that leave the app, opened by the platform.

import { Linking } from 'react-native';

import type { ExternalLinksPort } from '../domain/ports';

export function createLinkingExternalLinks(): ExternalLinksPort {
  return {
    async open(url: string) {
      await Linking.openURL(url);
    },
  };
}
