import type { ShowcaseItem } from '../catalog';

import {
  BadgeItem,
  ButtonItem,
  IconButtonItem,
  IconItem,
  SeparatorItem,
  SpinnerItem,
  TextItem,
} from './basics';
import {
  CodeBlockItem,
  ComposerItem,
  DiffViewItem,
  MarkdownItem,
} from './codingAgent';
import {
  ChipItem,
  SegmentedControlItem,
  SwitchItem,
  TextFieldItem,
} from './inputs';
import { AlertDialogItem, PromptDialogItem } from './overlays';
import { SheetExamples } from './sheets';
import { CardItem, ListItemsItem, StatTileItem } from './surfaces';

export const SHOWCASE_ITEMS: ShowcaseItem[] = [
  { id: 'text', group: 'basics', title: 'Text', Component: TextItem },
  { id: 'badge', group: 'basics', title: 'Badge', Component: BadgeItem },
  { id: 'button', group: 'basics', title: 'Button', Component: ButtonItem },
  {
    id: 'icon-button',
    group: 'basics',
    title: 'IconButton',
    Component: IconButtonItem,
  },
  { id: 'icon', group: 'basics', title: 'Icon', Component: IconItem },
  {
    id: 'separator',
    group: 'basics',
    title: 'Separator',
    Component: SeparatorItem,
  },
  { id: 'spinner', group: 'basics', title: 'Spinner', Component: SpinnerItem },
  {
    id: 'text-field',
    group: 'inputs',
    title: 'TextField',
    Component: TextFieldItem,
  },
  { id: 'switch', group: 'inputs', title: 'Switch', Component: SwitchItem },
  {
    id: 'segmented-control',
    group: 'inputs',
    title: 'SegmentedControl',
    Component: SegmentedControlItem,
  },
  { id: 'chip', group: 'inputs', title: 'Chip', Component: ChipItem },
  { id: 'card', group: 'surfaces', title: 'Card', Component: CardItem },
  {
    id: 'list',
    group: 'surfaces',
    title: 'InsetGroup and ListItem',
    Component: ListItemsItem,
  },
  {
    id: 'stat-tile',
    group: 'surfaces',
    title: 'StatTile',
    Component: StatTileItem,
  },
  {
    id: 'alert-dialog',
    group: 'overlays',
    title: 'AlertDialog',
    Component: AlertDialogItem,
  },
  {
    id: 'prompt-dialog',
    group: 'overlays',
    title: 'PromptDialog',
    Component: PromptDialogItem,
  },
  {
    id: 'sheet',
    group: 'overlays',
    title: 'Bottom sheet',
    Component: SheetExamples,
  },
  {
    id: 'code-block',
    group: 'coding-agent',
    title: 'CodeBlock',
    Component: CodeBlockItem,
  },
  {
    id: 'markdown',
    group: 'coding-agent',
    title: 'Markdown',
    Component: MarkdownItem,
  },
  {
    id: 'diff-view',
    group: 'coding-agent',
    title: 'DiffView',
    Component: DiffViewItem,
  },
  {
    id: 'composer',
    group: 'coding-agent',
    title: 'Composer',
    Component: ComposerItem,
  },
];
