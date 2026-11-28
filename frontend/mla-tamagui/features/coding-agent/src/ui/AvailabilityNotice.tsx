import { useSkin } from '@lib/skin';

export function AvailabilityNotice({ missing, reconnecting, onAction }: {
  missing?: boolean;
  reconnecting?: boolean;
  onAction: () => void;
}) {
  const { Notice } = useSkin();
  return (
    <Notice
      title={missing ? 'Worktree unavailable' : 'RCA server unreachable'}
      detail={missing ? 'This worktree may have been deleted or moved on the desktop.'
        : 'Check that the desktop is online and RCA is running.' + (reconnecting ? ' Reconnecting automatically.' : '')}
      actionLabel={missing ? 'View workspaces' : 'Reconnect now'}
      onAction={onAction}
    />
  );
}
