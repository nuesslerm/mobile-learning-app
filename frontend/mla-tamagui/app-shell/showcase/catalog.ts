import type { ComponentType } from 'react';

export type ShowcaseGroupId =
  | 'basics'
  | 'inputs'
  | 'surfaces'
  | 'overlays'
  | 'coding-agent';

export type ShowcaseItem = {
  id: string;
  group: ShowcaseGroupId;
  title: string;
  Component: ComponentType;
};

export const SHOWCASE_GROUPS: { id: ShowcaseGroupId; title: string }[] = [
  { id: 'basics', title: 'Basics' },
  { id: 'inputs', title: 'Inputs' },
  { id: 'surfaces', title: 'Lists and surfaces' },
  { id: 'overlays', title: 'Overlays' },
  { id: 'coding-agent', title: 'Coding agent' },
];
