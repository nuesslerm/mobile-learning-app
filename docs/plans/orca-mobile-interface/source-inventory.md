# Orca mobile source inventory

Reference commit `980c0024741812bd7ae934c0c22144d9926a784e`. Paths below are relative to its `mobile/` directory. Each route lists its statically reachable rendering modules. Module entries enumerate JSX controls, including inline native controls. Runtime visibility, platform conditions and captures require separate verification.

## Route modules

### `app/_layout.tsx`

Rendering modules: `app/_layout.tsx`, `src/components/NativeScreenRecovery.tsx`, `src/components/OrcaLogo.tsx`, `src/layout/side-rail.tsx`, `src/layout/side-safe-area.tsx`, `src/query/orca-query-provider.tsx`, `src/transport/connection/client-context.tsx`.

### `app/_layout.web.tsx`

Rendering modules: `app/_layout.web.tsx`, `src/components/detail-card-host.web.tsx`, `src/layout/side-safe-area.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/about.tsx`

Rendering modules: `app/about.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/OrcaLogo.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/settings/about-screen.tsx`, `src/settings/settings-screen-frame.tsx`.

### `app/appearance-settings.tsx`

Rendering modules: `src/components/DisclosureChevron.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/settings/appearance-settings-screen.tsx`, `src/settings/settings-screen-frame.tsx`, `src/settings/ui-language-settings-section.tsx`.

### `app/browser-settings.tsx`

Rendering modules: `src/components/AutoHeight.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileSearchField.tsx`, `src/components/PickerModal.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/layout/side-safe-area.tsx`, `src/settings/browser-settings-screen.tsx`, `src/settings/settings-screen-frame.tsx`.

### `app/connection-log.tsx`

Rendering modules: `app/connection-log.tsx`, `src/components/ConnectionLog.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/diagnostics/connection-diagnostics-screen.tsx`, `src/diagnostics/connection-diagnostics-view.tsx`, `src/settings/settings-screen-frame.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/[...page].tsx`

Rendering modules: `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/catch-all-page-route.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/accounts.tsx`

Rendering modules: `app/h/[hostId]/accounts.tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/accounts.web.tsx`

Rendering modules: `app/h/[hostId]/accounts.web.tsx`, `src/accounts/MobileAccountsLoadState.tsx`, `src/accounts/MobileAccountsScreen.tsx`, `src/accounts/ProviderUsageWindows.tsx`, `src/accounts/usage/CodexResetCreditAction.tsx`, `src/accounts/usage/UsageBar.tsx`, `src/components/AgentIcons.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/HostOfflineBanner.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileStateView.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/layout/side-safe-area.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/agent-history/[worktreeId].tsx`

Rendering modules: `app/h/[hostId]/agent-history/[worktreeId].tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/navigation/workspace-route.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/agent-history/[worktreeId].web.tsx`

Rendering modules: `app/h/[hostId]/agent-history/[worktreeId].web.tsx`, `src/agent-history/AgentHistoryNotices.tsx`, `src/agent-history/AgentHistorySessionRow.tsx`, `src/agent-history/MobileAgentSessionHistoryList.tsx`, `src/agent-history/MobileAgentSessionHistoryPanel.tsx`, `src/components/AgentIcons.tsx`, `src/components/AutoHeight.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileStateView.tsx`, `src/components/TransientToast.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/BottomSearchBar.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/navigation/workspace-route.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/automations.tsx`

Rendering modules: `app/h/[hostId]/automations.tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/automations.web.tsx`

Rendering modules: `app/h/[hostId]/automations.web.tsx`, `src/automations/AutomationAdvancedFields.tsx`, `src/automations/AutomationControls.tsx`, `src/automations/AutomationDetail.tsx`, `src/automations/AutomationDraftReview.tsx`, `src/automations/AutomationEditor.tsx`, `src/automations/AutomationEditorNavigation.tsx`, `src/automations/AutomationList.tsx`, `src/automations/AutomationManager.tsx`, `src/automations/AutomationRunDetails.tsx`, `src/automations/AutomationRunList.tsx`, `src/automations/AutomationScheduleFields.tsx`, `src/automations/MobileAutomationsScreen.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/OrcaHookTrustDrawer.tsx`, `src/components/PickerModal.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/FieldTextInput.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/layout/side-safe-area.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/edit.tsx`

Rendering modules: `app/h/[hostId]/edit.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/layout/side-safe-area.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/edit.web.tsx`

Rendering modules: `app/h/[hostId]/edit.web.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/catch-all-page-route.web.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/files/[worktreeId].tsx`

Rendering modules: `app/h/[hostId]/files/[worktreeId].tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/navigation/workspace-route.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/files/[worktreeId].web.tsx`

Rendering modules: `app/h/[hostId]/files/[worktreeId].web.tsx`, `src/components/ActionSheetModal.tsx`, `src/components/AutoHeight.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileStateView.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/BottomSearchBar.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/files/explorer/MobileFileExplorerPanel.tsx`, `src/files/explorer/mobile-file-explorer-row-actions-sheet.tsx`, `src/files/explorer/mobile-file-explorer-row.tsx`, `src/layout/side-safe-area.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/navigation/workspace-route.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/files/preview/[worktreeId].tsx`

Rendering modules: `app/h/[hostId]/files/preview/[worktreeId].tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/navigation/workspace-route.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/files/preview/[worktreeId].web.tsx`

Rendering modules: `app/h/[hostId]/files/preview/[worktreeId].web.tsx`, `src/components/ActionSheetModal.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileHtmlPreview.tsx`, `src/components/MobileMarkdown.tsx`, `src/components/MobileMarkdownCallout.tsx`, `src/components/MobileMarkdownCodeBlock.tsx`, `src/components/MobileMarkdownDetails.tsx`, `src/components/MobileMarkdownMath.tsx`, `src/components/MobileMarkdownRemoteImage.tsx`, `src/components/MobilePanZoomImage.tsx`, `src/components/MobilePreviewSourceTabs.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileSourceCopyButton.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSvgPreview.tsx`, `src/components/MobileSyntaxSegments.tsx`, `src/components/TransientToast.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/list/ListGroup.tsx`, `src/components/mobile-markdown-blocks.tsx`, `src/components/mobile-markdown-image-paragraph.tsx`, `src/components/mobile-markdown-inline.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/pr-sidebar/MermaidDiagram.tsx`, `src/drafts/DocumentEditDraftOfferBar.tsx`, `src/files/MobileFilePreviewScreen.tsx`, `src/files/attachments/AttachmentTextScrollView.tsx`, `src/files/attachments/MobileChatAttachmentPreview.tsx`, `src/files/markdown/MobileFileMarkdownPreview.tsx`, `src/files/markdown/MobileMarkdownPreviewImage.tsx`, `src/files/markdown/MobileMarkdownPreviewModeBar.tsx`, `src/files/media/MobileFileMediaPreview.tsx`, `src/files/media/MobileMediaPlayback.tsx`, `src/files/media/MobilePdfPreview.tsx`, `src/files/preview/MobileFilePreviewBody.tsx`, `src/files/preview/MobileFilePreviewDiscardModal.tsx`, `src/files/preview/MobileFilePreviewEditableSource.tsx`, `src/files/preview/MobileFilePreviewSourceText.tsx`, `src/files/terminal-artifacts/MobileFilePreviewEditDraft.tsx`, `src/layout/side-safe-area.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/navigation/workspace-route.tsx`, `src/session/native-chat/visual/MobileImageViewerActions.tsx`, `src/session/native-chat/visual/MobileNativeChatImageViewer.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/history/[worktreeId].tsx`

Rendering modules: `app/h/[hostId]/history/[worktreeId].tsx`.

### `app/h/[hostId]/index.tsx`

Rendering modules: `app/h/[hostId]/index.tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/index.web.tsx`

Rendering modules: `app/h/[hostId]/index.web.tsx`, `src/agent-history/MobileAgentSessionHistoryIcon.tsx`, `src/components/ActionSheetModal.tsx`, `src/components/AgentIcons.tsx`, `src/components/AgentSpinner.tsx`, `src/components/AgentStateDot.tsx`, `src/components/ArrivalFade.tsx`, `src/components/AuthFailedBanner.tsx`, `src/components/AuthFailedBannerActions.web.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/HostDiagnosticsLink.tsx`, `src/components/HostKindIcon.tsx`, `src/components/HostOfflineBanner.tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileRepoIcon.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSwitch.tsx`, `src/components/NewWorkspaceSetupScriptField.tsx`, `src/components/NewWorkspaceSshConnectionField.tsx`, `src/components/NewWorktreeFormOptions.tsx`, `src/components/NewWorktreeFormSheet.tsx`, `src/components/NewWorktreeModal.tsx`, `src/components/NewWorktreeModalController.tsx`, `src/components/NewWorktreeModalDrawers.tsx`, `src/components/NewWorktreeProjectTargetFields.tsx`, `src/components/OrcaHookTrustDrawer.tsx`, `src/components/PickerListDrawer.tsx`, `src/components/SheetPrimaryButton.tsx`, `src/components/SmartSourceModeIcon.tsx`, `src/components/SmartWorkspaceAdvancedFields.tsx`, `src/components/SmartWorkspaceSourceDrawer.tsx`, `src/components/SmartWorkspaceSourceField.tsx`, `src/components/SmartWorkspaceSourceRow.tsx`, `src/components/StatusDot.tsx`, `src/components/TaskProviderLogo.tsx`, `src/components/TextInputModal.tsx`, `src/components/WorkspaceDetailPlaceholder.tsx`, `src/components/WorktreeAgentChildDisclosure.tsx`, `src/components/WorktreeAgentList.tsx`, `src/components/WorktreeAgentRow.tsx`, `src/components/WorktreeAgentSummary.tsx`, `src/components/WorktreeListRow.tsx`, `src/components/WorktreeMetaGlyphs.tsx`, `src/components/WorktreeUnsavedName.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/bottom-drawer-modal-host.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/BottomSearchBar.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/nav-bar/PageTitle.tsx`, `src/components/nav-bar/PullDownMenu.tsx`, `src/components/nav-bar/SheetHeader.tsx`, `src/host-screen/HostScreen.tsx`, `src/host-screen/host-bottom-bar.tsx`, `src/host-screen/host-screen-header.tsx`, `src/host-screen/host-screen-overlays.tsx`, `src/host-screen/host-screen-view.tsx`, `src/host-screen/workspace-actions/workspace-details-sheet.tsx`, `src/host-screen/workspace-actions/workspace-rename-sheet.tsx`, `src/host-screen/workspace-list/host-pending-creation-row.tsx`, `src/host-screen/workspace-list/host-workspace-list.tsx`, `src/host-screen/workspace-options/workspace-option-controls.tsx`, `src/host-screen/workspace-options/workspace-option-rows.tsx`, `src/host-screen/workspace-options/workspace-options-sheet.tsx`, `src/layout/side-safe-area.tsx`, `src/transport/connection/client-context.web.tsx`, `src/worktree/host-workspace-list-states.tsx`.

### `app/h/[hostId]/new-workspace.tsx`

Rendering modules: `app/h/[hostId]/new-workspace.tsx`.

### `app/h/[hostId]/new-workspace.web.tsx`

Rendering modules: `app/h/[hostId]/new-workspace.web.tsx`, `src/components/AgentIcons.tsx`, `src/components/AutoHeight.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/HostKindIcon.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSwitch.tsx`, `src/components/NewWorkspaceSetupScriptField.tsx`, `src/components/NewWorkspaceSshConnectionField.tsx`, `src/components/NewWorktreeFormOptions.tsx`, `src/components/NewWorktreeFormSheet.tsx`, `src/components/NewWorktreeModal.tsx`, `src/components/NewWorktreeModalController.tsx`, `src/components/NewWorktreeModalDrawers.tsx`, `src/components/NewWorktreeProjectTargetFields.tsx`, `src/components/OrcaHookTrustDrawer.tsx`, `src/components/PickerListDrawer.tsx`, `src/components/SheetPrimaryButton.tsx`, `src/components/SmartSourceModeIcon.tsx`, `src/components/SmartWorkspaceAdvancedFields.tsx`, `src/components/SmartWorkspaceSourceDrawer.tsx`, `src/components/SmartWorkspaceSourceField.tsx`, `src/components/SmartWorkspaceSourceRow.tsx`, `src/components/TaskProviderLogo.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/bottom-drawer-modal-host.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/nav-bar/SheetHeader.tsx`, `src/layout/side-safe-area.tsx`, `src/new-workspace/NewWorkspacePageScreen.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/pr/[worktreeId].tsx`

Rendering modules: `app/h/[hostId]/pr/[worktreeId].tsx`.

### `app/h/[hostId]/review/[worktreeId].tsx`

Rendering modules: `app/h/[hostId]/review/[worktreeId].tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/navigation/workspace-route.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/review/[worktreeId].web.tsx`

Rendering modules: `app/h/[hostId]/review/[worktreeId].web.tsx`, `src/components/ActionSheetModal.tsx`, `src/components/AgentLaunchNotice.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileDiffReviewBody.tsx`, `src/components/MobileDiffReviewDrawers.tsx`, `src/components/MobileDiffReviewFileSummary.tsx`, `src/components/MobileDiffReviewFilesSheet.tsx`, `src/components/MobileDiffReviewFooter.tsx`, `src/components/MobileDiffReviewHeader.tsx`, `src/components/MobileDiffReviewLine.tsx`, `src/components/MobileDiffReviewLines.tsx`, `src/components/MobileDiffReviewRangeSheet.tsx`, `src/components/MobileDiffReviewScreenView.tsx`, `src/components/MobileMarkdown.tsx`, `src/components/MobileMarkdownCallout.tsx`, `src/components/MobileMarkdownCodeBlock.tsx`, `src/components/MobileMarkdownDetails.tsx`, `src/components/MobileMarkdownMath.tsx`, `src/components/MobileMarkdownRemoteImage.tsx`, `src/components/MobilePRSidebar.tsx`, `src/components/MobilePanZoomImage.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileSourceCopyButton.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSwitch.tsx`, `src/components/MobileSyntaxSegments.tsx`, `src/components/RightDrawer.tsx`, `src/components/SheetPrimaryButton.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/ComposerSendButton.tsx`, `src/components/composer-input/FieldTextInput.tsx`, `src/components/composer-input/MessageBox.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/mobile-markdown-blocks.tsx`, `src/components/mobile-markdown-image-paragraph.tsx`, `src/components/mobile-markdown-inline.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/nav-bar/PullDownMenu.tsx`, `src/components/nav-bar/SheetHeader.tsx`, `src/components/pr-sidebar/MermaidDiagram.tsx`, `src/components/pr-sidebar/MobileLinkPrForm.tsx`, `src/components/pr-sidebar/MobilePrTabSheets.tsx`, `src/components/pr-sidebar/PRCheckDetail.tsx`, `src/components/pr-sidebar/PRCommentCard.tsx`, `src/components/pr-sidebar/PRCommentComposer.tsx`, `src/components/pr-sidebar/PRCommentsSection.tsx`, `src/components/pr-sidebar/PRSection.tsx`, `src/components/pr-sidebar/PrBasePicker.tsx`, `src/components/pr-sidebar/PrCheckFailureRow.tsx`, `src/components/pr-sidebar/PrChecksScreen.tsx`, `src/components/pr-sidebar/PrCommentAvatar.tsx`, `src/components/pr-sidebar/PrCommentsScreen.tsx`, `src/components/pr-sidebar/PrCreateSheet.tsx`, `src/components/pr-sidebar/PrMarkdown.tsx`, `src/components/pr-sidebar/PrMiniButton.tsx`, `src/components/pr-sidebar/PrReviewersSheet.tsx`, `src/components/pr-sidebar/PrTabBottomAction.tsx`, `src/components/pr-sidebar/PrTabEmptyState.tsx`, `src/components/pr-sidebar/PrTabOverviewCards.tsx`, `src/components/pr-sidebar/PrTabScreenView.tsx`, `src/components/pr-sidebar/PrTextLink.tsx`, `src/components/pr-sidebar/PrThreadCode.tsx`, `src/components/pr-sidebar/PrThreadRow.tsx`, `src/components/pr-sidebar/PrThreadScreen.tsx`, `src/components/pr-sidebar/pr-tab-screen-frame.tsx`, `src/layout/side-safe-area.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/navigation/workspace-route.tsx`, `src/session/native-chat/visual/MobileNativeChatImageViewer.tsx`, `src/source-control/commits/MobileCommitFailurePanel.tsx`, `src/source-control/review/MobileDiffReviewRouteScreen.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/session/[worktreeId].tsx`

Rendering modules: `app/h/[hostId]/session/[worktreeId].tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/navigation/workspace-route.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/session/[worktreeId].web.tsx`

Rendering modules: `app/h/[hostId]/session/[worktreeId].web.tsx`, `src/agent-history/MobileAgentSessionHistoryIcon.tsx`, `src/browser/MobileBrowserPane.tsx`, `src/browser/pane-view/MobileBrowserAddressField.tsx`, `src/browser/pane-view/MobileBrowserKeyRow.tsx`, `src/browser/pane-view/MobileBrowserLoadFailure.tsx`, `src/browser/pane-view/MobileBrowserLoadProgress.tsx`, `src/browser/pane-view/MobileBrowserPaneView.tsx`, `src/browser/pane-view/MobileBrowserPointerModifiers.tsx`, `src/browser/pane-view/MobileBrowserToolbarIconButton.tsx`, `src/browser/pane-view/MobileBrowserViewModeSwitch.tsx`, `src/components/ActionSheetModal.tsx`, `src/components/AgentIcons.tsx`, `src/components/AgentLaunchNotice.tsx`, `src/components/AgentStateDot.tsx`, `src/components/AnchoredDetailCard.tsx`, `src/components/ArrivalFade.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/CustomKeyModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/DragReorderList.tsx`, `src/components/DragReorderRow.tsx`, `src/components/HostKindIcon.tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/LoadingSpinner.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileDictationSetupSheet.tsx`, `src/components/MobileDiffReviewFileSummary.tsx`, `src/components/MobileEdgeFade.tsx`, `src/components/MobileHtmlPreview.tsx`, `src/components/MobileMarkdown.tsx`, `src/components/MobileMarkdownCallout.tsx`, `src/components/MobileMarkdownCodeBlock.tsx`, `src/components/MobileMarkdownDetails.tsx`, `src/components/MobileMarkdownMath.tsx`, `src/components/MobileMarkdownRemoteImage.tsx`, `src/components/MobilePRSidebar.tsx`, `src/components/MobilePanZoomImage.tsx`, `src/components/MobilePreviewSourceTabs.tsx`, `src/components/MobileRichMarkdownEditor.tsx`, `src/components/MobileRichMarkdownToolbar.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileSourceCopyButton.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSvgPreview.tsx`, `src/components/MobileSwitch.tsx`, `src/components/MobileSyntaxSegments.tsx`, `src/components/NewWorkspaceSetupScriptField.tsx`, `src/components/NewWorkspaceSshConnectionField.tsx`, `src/components/NewWorktreeFormOptions.tsx`, `src/components/NewWorktreeFormSheet.tsx`, `src/components/NewWorktreeModal.tsx`, `src/components/NewWorktreeModalController.tsx`, `src/components/NewWorktreeModalDrawers.tsx`, `src/components/NewWorktreeProjectTargetFields.tsx`, `src/components/OrcaHookTrustDrawer.tsx`, `src/components/PickerListDrawer.tsx`, `src/components/PickerModal.tsx`, `src/components/SheetPrimaryButton.tsx`, `src/components/SmartRailRow.tsx`, `src/components/SmartSourceModeIcon.tsx`, `src/components/SmartWorkspaceAdvancedFields.tsx`, `src/components/SmartWorkspaceSourceDrawer.tsx`, `src/components/SmartWorkspaceSourceField.tsx`, `src/components/SmartWorkspaceSourceRow.tsx`, `src/components/StatusDot.tsx`, `src/components/SwipeRowActions.tsx`, `src/components/TailClippedText.tsx`, `src/components/TaskProviderLogo.tsx`, `src/components/TextInputModal.tsx`, `src/components/TransientToast.tsx`, `src/components/WorktreeMetaGlyphs.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/bottom-drawer-modal-host.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/ComposerSendButton.tsx`, `src/components/composer-input/FieldTextInput.tsx`, `src/components/composer-input/MessageBox.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/detail-card-host.web.tsx`, `src/components/list/ListGroup.tsx`, `src/components/mobile-markdown-blocks.tsx`, `src/components/mobile-markdown-image-paragraph.tsx`, `src/components/mobile-markdown-inline.tsx`, `src/components/nav-bar/BottomSearchBar.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/nav-bar/PullDownMenu.tsx`, `src/components/nav-bar/SheetHeader.tsx`, `src/components/pr-sidebar/MermaidDiagram.tsx`, `src/components/pr-sidebar/MobileLinkPrForm.tsx`, `src/components/pr-sidebar/MobilePrTabSheets.tsx`, `src/components/pr-sidebar/PRCheckDetail.tsx`, `src/components/pr-sidebar/PRCommentComposer.tsx`, `src/components/pr-sidebar/PrBasePicker.tsx`, `src/components/pr-sidebar/PrCheckFailureRow.tsx`, `src/components/pr-sidebar/PrChecksScreen.tsx`, `src/components/pr-sidebar/PrCommentAvatar.tsx`, `src/components/pr-sidebar/PrCommentsScreen.tsx`, `src/components/pr-sidebar/PrCreateSheet.tsx`, `src/components/pr-sidebar/PrMarkdown.tsx`, `src/components/pr-sidebar/PrMiniButton.tsx`, `src/components/pr-sidebar/PrReviewersSheet.tsx`, `src/components/pr-sidebar/PrTabBottomAction.tsx`, `src/components/pr-sidebar/PrTabEmptyState.tsx`, `src/components/pr-sidebar/PrTabOverviewCards.tsx`, `src/components/pr-sidebar/PrTabScreenView.tsx`, `src/components/pr-sidebar/PrTextLink.tsx`, `src/components/pr-sidebar/PrThreadCode.tsx`, `src/components/pr-sidebar/PrThreadRow.tsx`, `src/components/pr-sidebar/PrThreadScreen.tsx`, `src/components/pr-sidebar/pr-tab-screen-frame.tsx`, `src/dictation/MobileDictationLiveControls.tsx`, `src/dictation/MobileDictationMeter.tsx`, `src/dictation/MobileDictationStatusAnnouncement.tsx`, `src/dictation/mobile-dictation-host.tsx`, `src/drafts/DocumentEditDraftOfferBar.tsx`, `src/files/explorer/MobileFileExplorerPanel.tsx`, `src/files/explorer/mobile-file-explorer-row-actions-sheet.tsx`, `src/files/explorer/mobile-file-explorer-row.tsx`, `src/files/media/MobileFileMediaPreview.tsx`, `src/files/media/MobileMediaPlayback.tsx`, `src/files/media/MobilePdfPreview.tsx`, `src/files/session-documents/MobileSessionFileReader.tsx`, `src/files/session-documents/MobileSessionMarkdownReader.tsx`, `src/layout/side-safe-area.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/navigation/workspace-route.tsx`, `src/session/MobileNativeChatForkProvider.tsx`, `src/session/MobileSessionActiveContent.tsx`, `src/session/MobileSessionCommandDock.tsx`, `src/session/MobileSessionContentRow.tsx`, `src/session/MobileSessionContentState.tsx`, `src/session/MobileSessionHeader.tsx`, `src/session/MobileSessionHeaderMoreMenu.tsx`, `src/session/MobileSessionRouteScreen.tsx`, `src/session/MobileSessionSheets.tsx`, `src/session/MobileSessionSurface.tsx`, `src/session/MobileSessionTitleBlock.tsx`, `src/session/MobileSourceControlButton.tsx`, `src/session/SessionDockColumn.tsx`, `src/session/SessionDockPanelContent.tsx`, `src/session/native-chat/MobileNativeChatFloatingControls.tsx`, `src/session/native-chat/MobileNativeChatLowerRegion.web.tsx`, `src/session/native-chat/MobileNativeChatOverlay.tsx`, `src/session/native-chat/MobileNativeChatSendErrorBanner.tsx`, `src/session/native-chat/MobileNativeChatView.tsx`, `src/session/native-chat/composer/MobileNativeChatComposer.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerAllocation.web.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerAttachButton.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerAttachments.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerField.web.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerFieldCopy.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerMenu.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerMenuLayer.web.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerMic.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerModeChip.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerPlanChip.tsx`, `src/session/native-chat/composer/MobileNativeChatComposerSendSlot.tsx`, `src/session/native-chat/composer/MobileNativeChatContextUsageRing.tsx`, `src/session/native-chat/composer/MobileNativeChatDictationLeading.tsx`, `src/session/native-chat/composer/MobileNativeChatEditActions.tsx`, `src/session/native-chat/composer/MobileNativeChatInputLockStatus.tsx`, `src/session/native-chat/composer/MobileNativeChatSendButton.tsx`, `src/session/native-chat/input/MobileNativeChatAsk.tsx`, `src/session/native-chat/input/MobileNativeChatAskOptionRow.tsx`, `src/session/native-chat/input/MobileNativeChatBackgroundTasks.tsx`, `src/session/native-chat/input/MobileNativeChatCardHeaderAction.tsx`, `src/session/native-chat/input/MobileNativeChatDockedTaskList.tsx`, `src/session/native-chat/input/MobileNativeChatOfflineQueue.tsx`, `src/session/native-chat/input/MobileNativeChatOptionPreviewSheet.tsx`, `src/session/native-chat/input/MobileNativeChatOptionThumbnail.tsx`, `src/session/native-chat/input/MobileNativeChatPermission.tsx`, `src/session/native-chat/input/MobileNativeChatPromptCard.tsx`, `src/session/native-chat/input/MobileNativeChatPromptStrip.tsx`, `src/session/native-chat/input/MobileNativeChatQuestion.tsx`, `src/session/native-chat/input/MobileNativeChatQueueFrame.tsx`, `src/session/native-chat/input/MobileNativeChatQueuedMessages.tsx`, `src/session/native-chat/input/MobileNativeChatSessionOptionPickers.tsx`, `src/session/native-chat/input/MobileNativeChatSessionOptionRows.tsx`, `src/session/native-chat/input/MobileNativeChatSessionOptionSegments.tsx`, `src/session/native-chat/input/MobileNativeChatStopButton.tsx`, `src/session/native-chat/input/MobileNativeChatStructuredAsk.tsx`, `src/session/native-chat/input/MobileNativeChatTaskList.tsx`, `src/session/native-chat/input/MobileNativeChatThreadGoalStrip.tsx`, `src/session/native-chat/input/MobileQueuedCardText.tsx`, `src/session/native-chat/input/MobileQueuedMessageCardRow.tsx`, `src/session/native-chat/input/MobileQueuedMessageComposer.tsx`, `src/session/native-chat/input/MobileQueuedMessageScrollList.tsx`, `src/session/native-chat/input/mobile-native-chat-control-swap.tsx`, `src/session/native-chat/transcript/MobileAgentMessageProvider.tsx`, `src/session/native-chat/transcript/MobileAgentMessageSenders.tsx`, `src/session/native-chat/transcript/MobileLinkActionSheet.tsx`, `src/session/native-chat/transcript/MobileNativeChatAgentMessageCard.tsx`, `src/session/native-chat/transcript/MobileNativeChatAwaitingInputRow.tsx`, `src/session/native-chat/transcript/MobileNativeChatBackgroundTaskRun.tsx`, `src/session/native-chat/transcript/MobileNativeChatDiffCard.tsx`, `src/session/native-chat/transcript/MobileNativeChatEmptyStateView.tsx`, `src/session/native-chat/transcript/MobileNativeChatFullContent.tsx`, `src/session/native-chat/transcript/MobileNativeChatInterruptedContinueButton.tsx`, `src/session/native-chat/transcript/MobileNativeChatLiveLine.tsx`, `src/session/native-chat/transcript/MobileNativeChatLongPressContent.tsx`, `src/session/native-chat/transcript/MobileNativeChatMarkdown.tsx`, `src/session/native-chat/transcript/MobileNativeChatMessage.tsx`, `src/session/native-chat/transcript/MobileNativeChatMessageActionsSheet.tsx`, `src/session/native-chat/transcript/MobileNativeChatMessageMeta.tsx`, `src/session/native-chat/transcript/MobileNativeChatMessageTime.tsx`, `src/session/native-chat/transcript/MobileNativeChatNoticeDisclosure.tsx`, `src/session/native-chat/transcript/MobileNativeChatNoticePill.tsx`, `src/session/native-chat/transcript/MobileNativeChatNoticeRow.tsx`, `src/session/native-chat/transcript/MobileNativeChatOwnedScrollView.tsx`, `src/session/native-chat/transcript/MobileNativeChatPageList.tsx`, `src/session/native-chat/transcript/MobileNativeChatPageScroll.tsx`, `src/session/native-chat/transcript/MobileNativeChatProse.tsx`, `src/session/native-chat/transcript/MobileNativeChatPullRequestEventLine.tsx`, `src/session/native-chat/transcript/MobileNativeChatQuestionText.tsx`, `src/session/native-chat/transcript/MobileNativeChatReasoningRow.tsx`, `src/session/native-chat/transcript/MobileNativeChatResolutionReceipt.tsx`, `src/session/native-chat/transcript/MobileNativeChatResponseActions.tsx`, `src/session/native-chat/transcript/MobileNativeChatRewindButton.tsx`, `src/session/native-chat/transcript/MobileNativeChatRewindDialog.tsx`, `src/session/native-chat/transcript/MobileNativeChatRowItemSource.tsx`, `src/session/native-chat/transcript/MobileNativeChatRowList.tsx`, `src/session/native-chat/transcript/MobileNativeChatRowOverlaysHost.tsx`, `src/session/native-chat/transcript/MobileNativeChatSubagentGroup.tsx`, `src/session/native-chat/transcript/MobileNativeChatTextAttachmentActions.tsx`, `src/session/native-chat/transcript/MobileNativeChatToolIcon.tsx`, `src/session/native-chat/transcript/MobileNativeChatToolLine.tsx`, `src/session/native-chat/transcript/MobileNativeChatToolRun.tsx`, `src/session/native-chat/transcript/MobileNativeChatTurnDiffRollup.tsx`, `src/session/native-chat/transcript/MobileNativeChatTurnStatus.tsx`, `src/session/native-chat/transcript/MobileNativeChatUsageLimitStatus.tsx`, `src/session/native-chat/transcript/MobileNativeChatUserMessageFold.tsx`, `src/session/native-chat/transcript/mobile-native-chat-list-footer.tsx`, `src/session/native-chat/transcript/mobile-native-chat-list-header.tsx`, `src/session/native-chat/transcript/mobile-native-chat-pinch-zoom.tsx`, `src/session/native-chat/transcript/use-mobile-native-chat-row-renderer.tsx`, `src/session/native-chat/visual/MobileImageViewerActions.tsx`, `src/session/native-chat/visual/MobileNativeChatImageViewer.tsx`, `src/session/native-chat/visual/MobileNativeChatVisual.tsx`, `src/session/native-chat/visual/MobileNativeChatVisualFrame.tsx`, `src/session/tabs/MobileAgentSessionTabActionSheet.tsx`, `src/session/tabs/MobileBrowserTabActionSheet.tsx`, `src/session/tabs/MobileNewTabSheet.tsx`, `src/session/tabs/MobileSessionTabBar.tsx`, `src/session/tabs/MobileSessionTabMark.tsx`, `src/session/tabs/MobileSessionTabsSheet.tsx`, `src/session/tabs/MobileWorkerAutoCloseStrip.tsx`, `src/source-control/MobileSourceControlPanel.tsx`, `src/source-control/MobileSourceControlRoutePanel.tsx`, `src/source-control/changes/MobileSourceControlChangesHeader.tsx`, `src/source-control/changes/MobileSourceControlContent.tsx`, `src/source-control/changes/MobileSourceControlFileList.tsx`, `src/source-control/changes/MobileSourceControlFileRows.tsx`, `src/source-control/commits/MobileCommitFailurePanel.tsx`, `src/source-control/folder/MobileFolderEmptyState.tsx`, `src/source-control/folder/MobileFolderSourceControlPanel.tsx`, `src/source-control/folder/MobileFolderWorktreeRow.tsx`, `src/source-control/folder/use-folder-create-flow.tsx`, `src/source-control/history/MobileCommitDetailPage.tsx`, `src/source-control/history/MobileCommitListRows.tsx`, `src/source-control/history/MobileCommitsScreen.tsx`, `src/source-control/history/MobileGitHistoryList.tsx`, `src/source-control/panel/MobileSourceControlHeader.tsx`, `src/source-control/panel/MobileSourceControlHubCommitsPage.tsx`, `src/source-control/panel/MobileSourceControlModals.tsx`, `src/source-control/panel/MobileSourceControlNextStepBar.tsx`, `src/source-control/review/MobileSessionDiffLineRow.tsx`, `src/terminal/quick-commands/QuickCommandEditorForm.tsx`, `src/terminal/quick-commands/QuickCommandRow.tsx`, `src/terminal/quick-commands/QuickCommandsList.tsx`, `src/terminal/quick-commands/QuickCommandsSheet.tsx`, `src/terminal/session/accessory-keys/MobileSessionAccessoryKeys.tsx`, `src/terminal/session/accessory-keys/MobileSessionLatchKeys.tsx`, `src/terminal/session/accessory-keys/MobileSessionSmartKeys.tsx`, `src/terminal/session/accessory-keys/MobileTerminalMoreKeysSheet.tsx`, `src/terminal/session/accessory-keys/TerminalMoreKeysSheet.tsx`, `src/terminal/session/input-controls/MobileTerminalInputActions.tsx`, `src/terminal/session/input-controls/MobileTerminalLiveInputStatus.tsx`, `src/terminal/session/pane-layout/MobileSessionTerminalLayoutSheet.tsx`, `src/terminal/session/pane-layout/TerminalPaneView.tsx`, `src/terminal/session/tab-actions/MobileTerminalRenameModal.tsx`, `src/terminal/webview/TerminalWebView.web.tsx`, `src/terminal/webview/terminal-webview-engine-error-state.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/source-control/[worktreeId].tsx`

Rendering modules: `app/h/[hostId]/source-control/[worktreeId].tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/navigation/workspace-route.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/source-control/[worktreeId].web.tsx`

Rendering modules: `app/h/[hostId]/source-control/[worktreeId].web.tsx`, `src/components/AgentIcons.tsx`, `src/components/AgentLaunchNotice.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/HostKindIcon.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileDiffReviewFileSummary.tsx`, `src/components/MobileMarkdown.tsx`, `src/components/MobileMarkdownCallout.tsx`, `src/components/MobileMarkdownCodeBlock.tsx`, `src/components/MobileMarkdownDetails.tsx`, `src/components/MobileMarkdownMath.tsx`, `src/components/MobileMarkdownRemoteImage.tsx`, `src/components/MobilePRSidebar.tsx`, `src/components/MobilePanZoomImage.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileSourceCopyButton.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSwitch.tsx`, `src/components/MobileSyntaxSegments.tsx`, `src/components/NewWorkspaceSetupScriptField.tsx`, `src/components/NewWorkspaceSshConnectionField.tsx`, `src/components/NewWorktreeFormOptions.tsx`, `src/components/NewWorktreeFormSheet.tsx`, `src/components/NewWorktreeModal.tsx`, `src/components/NewWorktreeModalController.tsx`, `src/components/NewWorktreeModalDrawers.tsx`, `src/components/NewWorktreeProjectTargetFields.tsx`, `src/components/OrcaHookTrustDrawer.tsx`, `src/components/PickerListDrawer.tsx`, `src/components/PickerModal.tsx`, `src/components/SheetPrimaryButton.tsx`, `src/components/SmartSourceModeIcon.tsx`, `src/components/SmartWorkspaceAdvancedFields.tsx`, `src/components/SmartWorkspaceSourceDrawer.tsx`, `src/components/SmartWorkspaceSourceField.tsx`, `src/components/SmartWorkspaceSourceRow.tsx`, `src/components/SwipeRowActions.tsx`, `src/components/TaskProviderLogo.tsx`, `src/components/TransientToast.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/bottom-drawer-modal-host.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/ComposerSendButton.tsx`, `src/components/composer-input/FieldTextInput.tsx`, `src/components/composer-input/MessageBox.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/mobile-markdown-blocks.tsx`, `src/components/mobile-markdown-image-paragraph.tsx`, `src/components/mobile-markdown-inline.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/nav-bar/PullDownMenu.tsx`, `src/components/nav-bar/SheetHeader.tsx`, `src/components/pr-sidebar/MermaidDiagram.tsx`, `src/components/pr-sidebar/MobileLinkPrForm.tsx`, `src/components/pr-sidebar/MobilePrTabSheets.tsx`, `src/components/pr-sidebar/PRCheckDetail.tsx`, `src/components/pr-sidebar/PRCommentComposer.tsx`, `src/components/pr-sidebar/PrBasePicker.tsx`, `src/components/pr-sidebar/PrCheckFailureRow.tsx`, `src/components/pr-sidebar/PrChecksScreen.tsx`, `src/components/pr-sidebar/PrCommentAvatar.tsx`, `src/components/pr-sidebar/PrCommentsScreen.tsx`, `src/components/pr-sidebar/PrCreateSheet.tsx`, `src/components/pr-sidebar/PrMarkdown.tsx`, `src/components/pr-sidebar/PrMiniButton.tsx`, `src/components/pr-sidebar/PrReviewersSheet.tsx`, `src/components/pr-sidebar/PrTabBottomAction.tsx`, `src/components/pr-sidebar/PrTabEmptyState.tsx`, `src/components/pr-sidebar/PrTabOverviewCards.tsx`, `src/components/pr-sidebar/PrTabScreenView.tsx`, `src/components/pr-sidebar/PrTextLink.tsx`, `src/components/pr-sidebar/PrThreadCode.tsx`, `src/components/pr-sidebar/PrThreadRow.tsx`, `src/components/pr-sidebar/PrThreadScreen.tsx`, `src/components/pr-sidebar/pr-tab-screen-frame.tsx`, `src/layout/side-safe-area.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/navigation/workspace-route.tsx`, `src/session/native-chat/visual/MobileNativeChatImageViewer.tsx`, `src/source-control/MobileSourceControlPanel.tsx`, `src/source-control/MobileSourceControlRoutePanel.tsx`, `src/source-control/changes/MobileSourceControlChangesHeader.tsx`, `src/source-control/changes/MobileSourceControlContent.tsx`, `src/source-control/changes/MobileSourceControlFileList.tsx`, `src/source-control/changes/MobileSourceControlFileRows.tsx`, `src/source-control/commits/MobileCommitFailurePanel.tsx`, `src/source-control/folder/MobileFolderEmptyState.tsx`, `src/source-control/folder/MobileFolderSourceControlPanel.tsx`, `src/source-control/folder/MobileFolderWorktreeRow.tsx`, `src/source-control/folder/use-folder-create-flow.tsx`, `src/source-control/history/MobileCommitDetailPage.tsx`, `src/source-control/history/MobileCommitListRows.tsx`, `src/source-control/history/MobileCommitsScreen.tsx`, `src/source-control/history/MobileGitHistoryList.tsx`, `src/source-control/panel/MobileSourceControlHeader.tsx`, `src/source-control/panel/MobileSourceControlHubCommitsPage.tsx`, `src/source-control/panel/MobileSourceControlModals.tsx`, `src/source-control/panel/MobileSourceControlNextStepBar.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/[hostId]/tasks.tsx`

Rendering modules: `app/h/[hostId]/tasks.tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/layout/side-rail.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/transport/connection/client-context.tsx`.

### `app/h/[hostId]/tasks.web.tsx`

Rendering modules: `app/h/[hostId]/tasks.web.tsx`, `src/components/ActionSheetModal.tsx`, `src/components/AgentIcons.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/HostOfflineBanner.tsx`, `src/components/JiraConnectPrompt.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileMarkdown.tsx`, `src/components/MobileMarkdownCallout.tsx`, `src/components/MobileMarkdownCodeBlock.tsx`, `src/components/MobileMarkdownDetails.tsx`, `src/components/MobileMarkdownMath.tsx`, `src/components/MobileMarkdownRemoteImage.tsx`, `src/components/MobilePanZoomImage.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileSourceCopyButton.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSyntaxSegments.tsx`, `src/components/MobileWorkspaceNameInput.tsx`, `src/components/NewWorkspaceSshConnectionField.tsx`, `src/components/PickerModal.tsx`, `src/components/SheetPrimaryButton.tsx`, `src/components/StatusDot.tsx`, `src/components/TaskProviderLogo.tsx`, `src/components/WorktreeMetaGlyphs.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/bottom-drawer-modal-host.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/ComposerSendButton.tsx`, `src/components/composer-input/MessageBox.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/mobile-markdown-blocks.tsx`, `src/components/mobile-markdown-image-paragraph.tsx`, `src/components/mobile-markdown-inline.tsx`, `src/components/nav-bar/BottomSearchBar.tsx`, `src/components/nav-bar/FilterChip.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/nav-bar/SheetHeader.tsx`, `src/components/pr-sidebar/MermaidDiagram.tsx`, `src/layout/side-safe-area.tsx`, `src/session/native-chat/visual/MobileNativeChatImageViewer.tsx`, `src/tasks/MobileTasksLegacySurface.tsx`, `src/tasks/hosted-review/MobileTasksPrFileDiff.tsx`, `src/tasks/item-detail/mobile-tasks-item-actions.tsx`, `src/tasks/item-detail/mobile-tasks-item-comments.tsx`, `src/tasks/item-detail/mobile-tasks-item-detail-content.tsx`, `src/tasks/item-detail/mobile-tasks-item-detail-drawer.tsx`, `src/tasks/item-detail/mobile-tasks-item-field-editors.tsx`, `src/tasks/item-detail/mobile-tasks-item-review-sections.tsx`, `src/tasks/item-detail/use-mobile-tasks-detail-comment-renderers.tsx`, `src/tasks/project-board/mobile-tasks-github-project-list.tsx`, `src/tasks/project-board/mobile-tasks-github-project-pickers.tsx`, `src/tasks/project-board/mobile-tasks-project-comments.tsx`, `src/tasks/project-board/mobile-tasks-project-detail-content.tsx`, `src/tasks/project-board/mobile-tasks-project-detail-drawer.tsx`, `src/tasks/project-board/mobile-tasks-project-field-editors.tsx`, `src/tasks/project-board/mobile-tasks-project-metadata-editors.tsx`, `src/tasks/project-board/mobile-tasks-project-review-panels.tsx`, `src/tasks/project-board/use-mobile-tasks-picker-projection.tsx`, `src/tasks/project-board/use-mobile-tasks-workspace-create-projection.tsx`, `src/tasks/providers/jira/mobile-tasks-jira-view-controls.tsx`, `src/tasks/providers/linear/mobile-tasks-linear-list.tsx`, `src/tasks/providers/linear/mobile-tasks-linear-view-controls.tsx`, `src/tasks/screen/mobile-tasks-filter-pickers.tsx`, `src/tasks/screen/mobile-tasks-filter-sheet.tsx`, `src/tasks/screen/mobile-tasks-github-view-controls.tsx`, `src/tasks/screen/mobile-tasks-host-subtitle.tsx`, `src/tasks/screen/mobile-tasks-provider-controls.tsx`, `src/tasks/screen/mobile-tasks-provider-pickers.tsx`, `src/tasks/screen/mobile-tasks-screen-chrome.tsx`, `src/tasks/screen/mobile-tasks-search-control.tsx`, `src/tasks/task-list/mobile-tasks-list-display-pickers.tsx`, `src/tasks/task-list/mobile-tasks-list-state.tsx`, `src/tasks/task-list/mobile-tasks-list-surface.tsx`, `src/tasks/task-list/mobile-tasks-options.tsx`, `src/tasks/task-list/mobile-tasks-provider-item-list.tsx`, `src/tasks/workspace-creation/mobile-tasks-create-drawers.tsx`, `src/tasks/workspace-creation/mobile-tasks-workspace-create-drawer.tsx`, `src/tasks/workspace-creation/mobile-tasks-workspace-option-pickers.tsx`, `src/tasks/workspace-creation/mobile-tasks-workspace-trust-drawers.tsx`, `src/transport/connection/client-context.web.tsx`.

### `app/h/_layout.tsx`

Rendering modules: `app/h/_layout.tsx`, `src/agent-history/MobileAgentSessionHistoryIcon.tsx`, `src/components/ActionSheetModal.tsx`, `src/components/AgentIcons.tsx`, `src/components/AgentSpinner.tsx`, `src/components/AgentStateDot.tsx`, `src/components/ArrivalFade.tsx`, `src/components/AuthFailedBanner.tsx`, `src/components/AuthFailedBannerActions.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/HostDiagnosticsLink.tsx`, `src/components/HostKindIcon.tsx`, `src/components/HostOfflineBanner.tsx`, `src/components/HostProtocolGate.tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileRepoIcon.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSwitch.tsx`, `src/components/NewWorkspaceSetupScriptField.tsx`, `src/components/NewWorkspaceSshConnectionField.tsx`, `src/components/NewWorktreeFormOptions.tsx`, `src/components/NewWorktreeFormSheet.tsx`, `src/components/NewWorktreeModal.tsx`, `src/components/NewWorktreeModalController.tsx`, `src/components/NewWorktreeModalDrawers.tsx`, `src/components/NewWorktreeProjectTargetFields.tsx`, `src/components/OrcaHookTrustDrawer.tsx`, `src/components/PickerListDrawer.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/components/SheetPrimaryButton.tsx`, `src/components/SmartSourceModeIcon.tsx`, `src/components/SmartWorkspaceAdvancedFields.tsx`, `src/components/SmartWorkspaceSourceDrawer.tsx`, `src/components/SmartWorkspaceSourceField.tsx`, `src/components/SmartWorkspaceSourceRow.tsx`, `src/components/StatusDot.tsx`, `src/components/TaskProviderLogo.tsx`, `src/components/TextInputModal.tsx`, `src/components/WorktreeAgentChildDisclosure.tsx`, `src/components/WorktreeAgentList.tsx`, `src/components/WorktreeAgentRow.tsx`, `src/components/WorktreeAgentSummary.tsx`, `src/components/WorktreeListRow.tsx`, `src/components/WorktreeMetaGlyphs.tsx`, `src/components/WorktreeUnsavedName.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/bottom-drawer-modal-host.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/BottomSearchBar.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/nav-bar/PageTitle.tsx`, `src/components/nav-bar/PullDownMenu.tsx`, `src/components/nav-bar/SheetHeader.tsx`, `src/dictation/MobileDictationLiveControls.tsx`, `src/dictation/MobileDictationMeter.tsx`, `src/dictation/MobileDictationPill.tsx`, `src/dictation/MobileDictationStatusAnnouncement.tsx`, `src/dictation/mobile-dictation-host.tsx`, `src/host-screen/HostScreen.tsx`, `src/host-screen/host-bottom-bar.tsx`, `src/host-screen/host-screen-header.tsx`, `src/host-screen/host-screen-overlays.tsx`, `src/host-screen/host-screen-view.tsx`, `src/host-screen/workspace-actions/workspace-details-sheet.tsx`, `src/host-screen/workspace-actions/workspace-rename-sheet.tsx`, `src/host-screen/workspace-list/host-pending-creation-row.tsx`, `src/host-screen/workspace-list/host-workspace-list.tsx`, `src/host-screen/workspace-options/workspace-option-controls.tsx`, `src/host-screen/workspace-options/workspace-option-rows.tsx`, `src/host-screen/workspace-options/workspace-options-sheet.tsx`, `src/layout/side-safe-area.tsx`, `src/navigation/host-stack.tsx`, `src/transport/connection/client-context.tsx`, `src/worktree/host-workspace-list-states.tsx`.

### `app/index.tsx`

Rendering modules: `src/accounts/usage/UsageBar.tsx`, `src/agents/MobileAgentListRows.tsx`, `src/agents/use-agent-list-row-actions.tsx`, `src/components/ActionSheetModal.tsx`, `src/components/AgentIcons.tsx`, `src/components/AgentSpinner.tsx`, `src/components/AgentStateDot.tsx`, `src/components/ArrivalFade.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConfirmModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileAgentIcon.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSwitch.tsx`, `src/components/OrcaLogo.tsx`, `src/components/PickerModal.tsx`, `src/components/StatusDot.tsx`, `src/components/SwipeRowActions.tsx`, `src/components/TransientToast.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/components/nav-bar/PullDownMenu.tsx`, `src/home/AppUpdateCard.tsx`, `src/home/MobileHomeActiveAgents.tsx`, `src/home/MobileHomeAppUpdateCard.tsx`, `src/home/MobileHomeDesktopRow.tsx`, `src/home/MobileHomeEmptyState.tsx`, `src/home/MobileHomeHostList.tsx`, `src/home/MobileHomeScreen.tsx`, `src/home/MobileHomeSectionHeading.tsx`, `src/home/MobileHomeStats.tsx`, `src/home/MobileHomeTasksSection.tsx`, `src/home/tabs/MobileHomeAgentsDesktopFilter.tsx`, `src/home/tabs/MobileHomeAgentsTab.tsx`, `src/home/tabs/MobileHomeAutomationRows.tsx`, `src/home/tabs/MobileHomeAutomationsTab.tsx`, `src/home/tabs/MobileHomeFab.tsx`, `src/home/tabs/MobileHomeFeedTab.tsx`, `src/home/tabs/MobileHomeHostPill.tsx`, `src/home/tabs/MobileHomeSettingsTab.tsx`, `src/home/tabs/MobileHomeTabBar.tsx`, `src/home/tabs/MobileHomeTabHeader.tsx`, `src/home/tabs/MobileHomeUsageTab.tsx`, `src/layout/side-safe-area.tsx`, `src/settings/app-update-settings-rows.tsx`, `src/settings/mobile-settings-menu.tsx`, `src/settings/pending-credential-cleanup-card.tsx`, `src/settings/settings-app-update-section.tsx`, `src/settings/settings-menu-screen.tsx`, `src/settings/settings-screen-frame.tsx`, `src/transport/connection/client-context.tsx`.

### `app/mobile-onboarding.tsx`

Rendering modules: `app/mobile-onboarding.tsx`, `src/components/OrcaLogo.tsx`, `src/onboarding/MobileOnboardingPage.tsx`, `src/onboarding/NotificationOnboardingPreview.tsx`.

### `app/native-chat-settings.tsx`

Rendering modules: `src/components/DisclosureChevron.tsx`, `src/components/MobileSwitch.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/settings/native-chat-settings-screen.tsx`, `src/settings/settings-screen-frame.tsx`.

### `app/new-workspace.tsx`

Rendering modules: `app/new-workspace.tsx`, `src/components/AutoHeight.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/HostProtocolGate.tsx`, `src/components/HostRouteNoticeBanner.tsx`, `src/components/MobileSearchField.tsx`, `src/components/PickerModal.tsx`, `src/components/ProtocolBlockScreen.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/layout/side-rail.tsx`, `src/layout/side-safe-area.tsx`, `src/mobile-web-shell/DownloadProgressBar.tsx`, `src/mobile-web-shell/MobileWebShellScreen.tsx`, `src/mobile-web-shell/PageRouteUnavailableScreen.tsx`, `src/mobile-web-shell/ShellDevFacts.tsx`, `src/mobile-web-shell/ShellDownloadProgress.tsx`, `src/mobile-web-shell/ShellStateScreen.tsx`, `src/mobile-web-shell/ShellWaitingFrame.tsx`, `src/mobile-web-shell/terminal-direction-pad.tsx`, `src/mobile-web-shell/terminal-key-rail.tsx`, `src/mobile-web-shell/use-shell-key-rail.tsx`, `src/new-workspace/NewWorkspaceHostPicker.tsx`, `src/new-workspace/NewWorkspaceLoadingDrawer.tsx`, `src/new-workspace/NewWorkspaceOverlay.tsx`, `src/transport/connection/client-context.tsx`.

### `app/notification-opt-in.tsx`

Rendering modules: `app/notification-opt-in.tsx`.

### `app/notifications.tsx`

Rendering modules: `app/notifications.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileSwitch.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/notifications/delivery-policy/NotificationDeliverySection.tsx`, `src/settings/native-notification-delivery-settings.tsx`, `src/settings/notification-display-test.tsx`, `src/settings/notification-settings-screen.tsx`, `src/settings/settings-screen-frame.tsx`, `src/transport/connection/client-context.tsx`.

### `app/pair-confirm.tsx`

Rendering modules: `app/pair-confirm.tsx`, `src/components/ConnectionLog.tsx`, `src/pairing/pairing-visuals.tsx`, `src/transport/connection/client-context.tsx`.

### `app/pair-scan.tsx`

Rendering modules: `app/pair-scan.tsx`, `src/components/AutoHeight.tsx`, `src/components/ConnectionLog.tsx`, `src/components/TextInputModal.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/TextField.tsx`, `src/layout/side-safe-area.tsx`, `src/pairing/pairing-visuals.tsx`, `src/transport/connection/client-context.tsx`.

### `app/pair.tsx`

Rendering modules: `app/pair.tsx`.

### `app/settings.tsx`

Rendering modules: `app/settings.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/settings/app-update-settings-rows.tsx`, `src/settings/mobile-settings-menu.tsx`, `src/settings/pending-credential-cleanup-card.tsx`, `src/settings/settings-app-update-section.tsx`, `src/settings/settings-menu-screen.tsx`, `src/settings/settings-screen-frame.tsx`.

### `app/terminal-settings.tsx`

Rendering modules: `app/terminal-settings.tsx`, `src/components/AutoHeight.tsx`, `src/components/CustomKeyModal.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/DragReorderList.tsx`, `src/components/DragReorderRow.tsx`, `src/components/MobileSearchField.tsx`, `src/components/MobileSwitch.tsx`, `src/components/PickerModal.tsx`, `src/components/SmartRailRow.tsx`, `src/components/TerminalShortcutSettings.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/composer-input/FieldTextInput.tsx`, `src/components/composer-input/TextField.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/layout/side-rail.tsx`, `src/layout/side-safe-area.tsx`, `src/settings/settings-screen-frame.tsx`, `src/transport/connection/client-context.tsx`.

### `app/troubleshoot.tsx`

Rendering modules: `app/troubleshoot.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/diagnostics/mobile-web-bundle-probe-row.tsx`, `src/diagnostics/mobile-web-shell-update-failure-row.tsx`, `src/diagnostics/troubleshoot-view.tsx`, `src/settings/settings-screen-frame.tsx`, `src/transport/connection/client-context.tsx`.

### `app/voice-settings.tsx`

Rendering modules: `app/voice-settings.tsx`, `src/components/AutoHeight.tsx`, `src/components/DisclosureChevron.tsx`, `src/components/MobileSegmentedControl.tsx`, `src/components/MobileStateView.tsx`, `src/components/MobileSwitch.tsx`, `src/components/VoiceModelList.tsx`, `src/components/bottom-drawer/BottomDrawer.tsx`, `src/components/bottom-drawer/keyed-bottom-drawer.tsx`, `src/components/bottom-drawer/mounted-bottom-drawer.tsx`, `src/components/list/ListGroup.tsx`, `src/components/nav-bar/NavBar.tsx`, `src/components/nav-bar/NavBarButton.tsx`, `src/layout/side-safe-area.tsx`, `src/settings/settings-screen-frame.tsx`, `src/settings/voice-settings-screen.tsx`, `src/settings/voice-transcribe-on-section.tsx`, `src/transport/connection/client-context.tsx`.

## Rendering modules and controls

- `app/_layout.tsx` — OrcaLogo, OrcaQueryProvider, RpcClientProvider, SideRailProvider, SideSafeArea, Stack, Stack.Screen, StatusBar, View
- `app/_layout.web.tsx` — DetailCardHostProvider, SafeAreaFrameContext.Provider, SafeAreaInsetsContext.Provider, SideSafeArea, Slot, View
- `app/about.tsx` — AboutScreen
- `app/connection-log.tsx` — ConnectionDiagnosticsScreen, Pressable, Text, View
- `app/h/[hostId]/accounts.tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/accounts.web.tsx` — MobileAccountsScreen
- `app/h/[hostId]/agent-history/[worktreeId].tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/agent-history/[worktreeId].web.tsx` — MobileAgentSessionHistoryPanel
- `app/h/[hostId]/automations.tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/automations.web.tsx` — MobileAutomationsScreen
- `app/h/[hostId]/edit.tsx` — ActivityIndicator, ConfirmModal, KeyboardAvoidingView, ListGroup, ListRow, NavBar, NavBarButton, Pressable, ScrollView, Text, TextField, View
- `app/h/[hostId]/edit.web.tsx` — CatchAllPageRoute
- `app/h/[hostId]/files/[worktreeId].tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/files/[worktreeId].web.tsx` — MobileFileExplorerPanel
- `app/h/[hostId]/files/preview/[worktreeId].tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/files/preview/[worktreeId].web.tsx` — MobileFilePreviewScreen
- `app/h/[hostId]/history/[worktreeId].tsx` — Redirect
- `app/h/[hostId]/index.tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/index.web.tsx` — HostScreen, WorkspaceDetailPlaceholder
- `app/h/[hostId]/new-workspace.tsx` — Redirect
- `app/h/[hostId]/new-workspace.web.tsx` — NewWorkspacePageScreen
- `app/h/[hostId]/pr/[worktreeId].tsx` — Redirect
- `app/h/[hostId]/review/[worktreeId].tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/review/[worktreeId].web.tsx` — MobileDiffReviewRouteScreen
- `app/h/[hostId]/session/[worktreeId].tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/session/[worktreeId].web.tsx` — MobileSessionRouteScreen
- `app/h/[hostId]/source-control/[worktreeId].tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/source-control/[worktreeId].web.tsx` — MobileSourceControlRoutePanel
- `app/h/[hostId]/tasks.tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `app/h/[hostId]/tasks.web.tsx` — MobileTasksScreen
- `app/h/_layout.tsx` — HostAreaServingContext.Provider, HostProtocolGate, HostScreen, HostSidebarRevealContext.Provider, HostStack, MobileDictationHostProvider, MobileDictationPillLayer, View
- `app/mobile-onboarding.tsx` — Animated.View, MobileOnboardingFlow, MobileOnboardingPage, OrcaLogo, SafeAreaView, Text, View
- `app/new-workspace.tsx` — NewWorkspaceHostPicker, NewWorkspaceOverlay
- `app/notification-opt-in.tsx` — Redirect
- `app/notifications.tsx` — NativeNotificationDeliverySettings, NotificationDisplayTest, NotificationsScreen
- `app/pair-confirm.tsx` — ChevronLeft, ConnectionLog, PairConfirmContent, PairingProgress, Pressable, Text, View
- `app/pair-scan.tsx` — ActivityIndicator, CameraView, ChevronLeft, ClipboardIcon, ConnectionLog, PairingProgress, Pressable, QrCode, ScanReticle, Step, Text, TextInputModal, View
- `app/pair.tsx` — ActivityIndicator, Pressable, Text, View
- `app/settings.tsx` — PendingCredentialCleanupCard, SettingsAppUpdateSection, SettingsMenuScreen
- `app/terminal-settings.tsx` — Animated.ScrollView, GestureHandlerRootView, HostFitRow, ListRow, PickerModal, SafeAreaView, SettingsEmptyRow, SettingsNavBar, SettingsSection, Smartphone, TerminalShortcutSettings, Type
- `app/troubleshoot.tsx` — MobileWebBundleProbeRow, MobileWebShellUpdateFailureRow, TroubleshootView
- `app/voice-settings.tsx` — VoiceSettingsScreen
- `src/accounts/MobileAccountsLoadState.tsx` — MobileLoadErrorState, MobileOfflineState, MobileStateView
- `src/accounts/MobileAccountsScreen.tsx` — ActivityIndicator, CodexResetCreditAction, ConfirmModal, HostOfflineBanner, Icon, ListGroup, ListRow, MobileAccountsLoadState, MobileAgentIcon, NavBar, NavBarButton, ProviderUsageWindows, SafeAreaView, ScrollView, SectionHeader, Text, User, View
- `src/accounts/ProviderUsageWindows.tsx` — UsageBar, View
- `src/accounts/usage/CodexResetCreditAction.tsx` — ActivityIndicator, ListRow, Pressable, RotateCcw, Text
- `src/accounts/usage/UsageBar.tsx` — ActivityIndicator, Text, View
- `src/agent-history/AgentHistoryNotices.tsx` — AgentHistorySkippedNotice, AutoHeight, Pressable, ScrollView, Text, TriangleAlert, View, X
- `src/agent-history/AgentHistorySessionRow.tsx` — ActivityIndicator, AutoHeight, ChevronDown, DisclosureChevron, MobileAgentIcon, Play, Pressable, PreviewTurns, Text, View
- `src/agent-history/MobileAgentSessionHistoryIcon.tsx` — Path, Svg
- `src/agent-history/MobileAgentSessionHistoryList.tsx` — AgentHistorySessionRow, RefreshControl, SectionHeader, SectionList, View
- `src/agent-history/MobileAgentSessionHistoryPanel.tsx` — AgentHistoryResumeNotice, AgentHistoryScanNotices, BottomSearchBar, MobileAgentSessionHistoryList, MobileAgentSessionHistoryPanelContent, MobileLoadErrorState, MobileNoMatchState, MobileOfflineState, MobileSegmentedControl, MobileStateView, NavBar, NavBarButton, SafeAreaView, TransientToast, View
- `src/agents/MobileAgentListRows.tsx` — AgentSpinner, Bell, ChevronDown, ChevronRight, Fragment, MobileAgentIcon, MobileAgentListRow, Pressable, SwipeRowActions, Text, View
- `src/agents/use-agent-list-row-actions.tsx` — ActionSheetModal, TransientToast
- `src/automations/AutomationAdvancedFields.tsx` — AutomationChoice, AutomationField, Switch, Text, View
- `src/automations/AutomationControls.tsx` — AutomationButton, FieldTextInput, MobileSegmentedControl, PickerModal, Pressable, Text, View
- `src/automations/AutomationDetail.tsx` — AutomationButton, AutomationMetadata, Switch, Text, View
- `src/automations/AutomationDraftReview.tsx` — AutomationMetadata, Text, View
- `src/automations/AutomationEditor.tsx` — AutomationAdvancedFields, AutomationButton, AutomationChoice, AutomationDraftReview, AutomationEditorHeader, AutomationEditorNavigation, AutomationField, AutomationScheduleFields, Text, View
- `src/automations/AutomationEditorNavigation.tsx` — AutomationButton, ConfirmModal, Text, View
- `src/automations/AutomationList.tsx` — AutomationButton, CalendarClock, ChevronRight, Pressable, Switch, Text, View
- `src/automations/AutomationManager.tsx` — AutomationButton, AutomationChoice, AutomationDetail, AutomationEditor, AutomationList, AutomationRunDetails, AutomationRunList, AutomationTabs, ConfirmModal, OrcaHookTrustDrawer, ScrollView, Text, View
- `src/automations/AutomationRunDetails.tsx` — AutomationButton, AutomationMetadata, Text, View
- `src/automations/AutomationRunList.tsx` — AutomationButton, ChevronRight, Pressable, Text, View
- `src/automations/AutomationScheduleFields.tsx` — AutomationChoice, AutomationField, Text, View
- `src/automations/MobileAutomationsScreen.tsx` — AutomationManager, NavBar, NavBarButton, NavBarTitleButton, PickerModal, View
- `src/browser/MobileBrowserPane.tsx` — MobileBrowserPaneView
- `src/browser/pane-view/MobileBrowserAddressField.tsx` — MobileBrowserLoadProgress, Text, TextField, View
- `src/browser/pane-view/MobileBrowserKeyRow.tsx` — Pressable, Text, View
- `src/browser/pane-view/MobileBrowserLoadFailure.tsx` — Text, View
- `src/browser/pane-view/MobileBrowserLoadProgress.tsx` — Animated.View, View
- `src/browser/pane-view/MobileBrowserPaneView.tsx` — ActivityIndicator, ArrowUp, ChevronLeft, ChevronRight, FieldTextInput, Image, MobileBrowserAddressField, MobileBrowserKeyRow, MobileBrowserLoadFailure, MobileBrowserPointerModifiers, MobileBrowserToolbarIconButton, MobileBrowserViewModeSwitch, Pressable, RefreshCw, Text, View
- `src/browser/pane-view/MobileBrowserPointerModifiers.tsx` — Pressable, Text, View
- `src/browser/pane-view/MobileBrowserToolbarIconButton.tsx` — Pressable
- `src/browser/pane-view/MobileBrowserViewModeSwitch.tsx` — Icon, Pressable, View, ViewModeButton
- `src/components/ActionSheetModal.tsx` — ActionSheetContent, ActivityIndicator, BottomDrawer, Icon, ListGroup, ListRow, Text, View
- `src/components/AgentIcons.tsx` — Path, Svg
- `src/components/AgentLaunchNotice.tsx` — Pressable, Text, View
- `src/components/AgentSpinner.tsx` — Activity, AgentStatusGlyph, ArrivalFade, ArrivingAgentStatus, View, WorkingRing
- `src/components/AgentStateDot.tsx` — Activity, Animated.View, View, WorkingRing
- `src/components/AnchoredDetailCard.tsx` — AnchoredDetailCard, Animated.View, DetailCardFocusBoundary, DetailCardLayer, ScrollView, Text, View
- `src/components/ArrivalFade.tsx` — Animated.View
- `src/components/AuthFailedBanner.tsx` — AuthFailedBannerActions, Text, View
- `src/components/AuthFailedBannerActions.tsx` — Pressable, Text
- `src/components/AuthFailedBannerActions.web.tsx` — Pressable, Text
- `src/components/AutoHeight.tsx` — Animated.View, AutoHeight, View
- `src/components/ConfirmModal.tsx` — BottomDrawer, ConfirmContent, Pressable, Text, View
- `src/components/ConnectionLog.tsx` — ScrollView, Text, View
- `src/components/CustomKeyModal.tsx` — BottomDrawer, ChevronLeft, FieldTextInput, ListGroup, ListRow, MobileSwitch, Pressable, SmartRailRow, Text, TextField, View
- `src/components/DisclosureChevron.tsx` — Animated.View
- `src/components/DragReorderList.tsx` — DragReorderRow, View
- `src/components/DragReorderRow.tsx` — Animated.View, GestureDetector, GripVertical, View
- `src/components/HostDiagnosticsLink.tsx` — ChevronRight, Pressable, Text
- `src/components/HostKindIcon.tsx` — Icon
- `src/components/HostOfflineBanner.tsx` — ActivityIndicator, CloudOff, Pressable, Text, View
- `src/components/HostProtocolGate.tsx` — ActivityIndicator, HostStatusGatesContext.Provider, ProtocolBlockScreen, View
- `src/components/HostRouteNoticeBanner.tsx` — Pressable, Text, View, X
- `src/components/JiraConnectPrompt.tsx` — MobileStateView, TaskProviderLogo
- `src/components/LoadingSpinner.tsx` — Animated.View, Hourglass, LoaderCircle
- `src/components/MobileAgentIcon.tsx` — AgentLetterIcon, AiderIcon, BundledIcon, ClaudeIcon, Defs, FaviconIcon, G, Image, LinearGradient, OmpIcon, OpenAIIcon, Path, PiIcon, Stop, Svg, Terminal, Text, View
- `src/components/MobileDictationSetupSheet.tsx` — ActivityIndicator, BottomDrawer, Check, Download, MobileSwitch, Pressable, Text, View
- `src/components/MobileDiffReviewBody.tsx` — DiffUnavailableState, MobileDiffReviewLines, MobileLoadErrorState, MobileOfflineState, MobileStateView
- `src/components/MobileDiffReviewDrawers.tsx` — ActionSheetContent, CompletionContent, ConfirmContent, DeleteNoteButton, KeyboardAvoidingView, KeyedBottomDrawer, MessageBox, MobileDiffReviewFilesSheet, MobileDiffReviewRangeSheet, NoteComposerContent, Pressable, ReviewSheetContent, Send, SendNotesContent, Text, Trash2, View, X
- `src/components/MobileDiffReviewFileSummary.tsx` — Check, MobileDiffLineStats, Pressable, Text, View, ViewedCheckbox
- `src/components/MobileDiffReviewFilesSheet.tsx` — ChevronDown, DisclosureChevron, FileRow, FlatList, FolderRow, MessageSquare, MobileDiffLineStats, MobileSearchField, Pressable, SheetHeader, Text, View, ViewedCheckbox
- `src/components/MobileDiffReviewFooter.tsx` — ActivityIndicator, ChevronLeft, ChevronRight, Pressable, PrimaryIcon, Text, View
- `src/components/MobileDiffReviewHeader.tsx` — NavBar, NavBarButton, NavBarTitleButton, PullDownMenu
- `src/components/MobileDiffReviewLine.tsx` — MessageSquare, MobileSyntaxSegments, Pressable, Text, View
- `src/components/MobileDiffReviewLines.tsx` — CommentGroupView, FlatList, GestureDetector, MobileDiffReviewCharWidthProbe, MobileDiffReviewLine, Pressable, Text, UnfoldVertical, View
- `src/components/MobileDiffReviewRangeSheet.tsx` — KeyboardAvoidingView, Pressable, SheetHeader, Sparkles, Text, TextField, View
- `src/components/MobileDiffReviewScreenView.tsx` — MobileDiffReviewBody, MobileDiffReviewDrawers, MobileDiffReviewFileSummary, MobileDiffReviewFooter, MobileDiffReviewHeader, MobilePRSidebar, RightDrawer, SafeAreaView, Text, View
- `src/components/MobileEdgeFade.tsx` — Defs, LinearGradient, Rect, Stop, Svg, View
- `src/components/MobileHtmlPreview.tsx` — MobilePreviewSourceTabs, PreviewFrame, View, iframe
- `src/components/MobileMarkdown.tsx` — MarkdownText, MarkdownTextContext.Provider, MobileMarkdownContent, MobileMarkdownRemoteImage, View
- `src/components/MobileMarkdownCallout.tsx` — CalloutIcon, Info, Lightbulb, MessageSquareWarning, OctagonAlert, Text, TriangleAlert, View
- `src/components/MobileMarkdownCodeBlock.tsx` — MarkdownText, MobileSourceCopyButton, MobileSyntaxSegments, Pressable, ScrollView, Text, TextWrap, View
- `src/components/MobileMarkdownDetails.tsx` — ChevronRight, DisclosureChevron, Pressable, Text, View
- `src/components/MobileMarkdownMath.tsx` — ScrollView, Text
- `src/components/MobileMarkdownRemoteImage.tsx` — Image, Pressable, Text
- `src/components/MobilePRSidebar.tsx` — ActivityIndicator, ArrowRight, GateIcon, Icon, ListGroup, ListRow, MobileLoadErrorState, MobilePrTabBody, MobilePrTabSheets, MobileStateView, NavBarButton, PrChangesCard, PrDescription, PrStateIcon, PrTabBottomAction, PrTabEmptyState, PrTabHeader, PrTabScreenView, PrTabSections, Pressable, PullDownMenu, RotateCw, ScrollView, Text, View
- `src/components/MobilePanZoomImage.tsx` — div, img
- `src/components/MobilePreviewSourceTabs.tsx` — Code, Eye, Pressable, Text, View
- `src/components/MobileRepoIcon.tsx` — Icon, Image, Text, View
- `src/components/MobileRichMarkdownEditor.tsx` — MobileRichMarkdownToolbar, TextInputModal, View
- `src/components/MobileRichMarkdownToolbar.tsx` — Icon, Pressable, ScrollView, View
- `src/components/MobileSearchField.tsx` — Pressable, Search, TextField, View, X
- `src/components/MobileSegmentedControl.tsx` — Pressable, ScrollView, Text, View
- `src/components/MobileSourceCopyButton.tsx` — Icon, Pressable, Text
- `src/components/MobileStateView.tsx` — ActivityIndicator, Icon, MobileStateButton, MobileStateDetails, MobileStateView, Pressable, Text, View
- `src/components/MobileSvgPreview.tsx` — MobileLoadErrorState, MobilePanZoomImage, MobilePreviewSourceTabs, View
- `src/components/MobileSwitch.tsx` — Animated.View, Pressable
- `src/components/MobileSyntaxSegments.tsx` — Text
- `src/components/MobileWorkspaceNameInput.tsx` — TextField
- `src/components/NativeScreenRecovery.tsx` — Pressable, Text, View
- `src/components/NewWorkspaceSetupScriptField.tsx` — ListGroup, ListRow, MobileSwitch, Text, View
- `src/components/NewWorkspaceSshConnectionField.tsx` — Pressable, Text, View
- `src/components/NewWorktreeFormOptions.tsx` — AutoHeight, NewWorkspaceSetupScriptField, SectionHeader, SmartWorkspaceAdvancedFields, Text, TextField, View
- `src/components/NewWorktreeFormSheet.tsx` — BottomDrawer, ListGroup, ListRow, MobileAgentIcon, MobileLoadErrorState, MobileStateView, NewWorkspaceSshConnectionField, NewWorktreeFormOptions, NewWorktreeProjectTargetFields, SectionHeader, SheetHeader, SheetPrimaryButton, SmartWorkspaceSourceField, Text, View
- `src/components/NewWorktreeModal.tsx` — BottomDrawerModalHost, NewWorktreeFormSheet, NewWorktreeModalContent, NewWorktreeModalDrawers
- `src/components/NewWorktreeModalController.tsx` — NewWorktreeModal
- `src/components/NewWorktreeModalDrawers.tsx` — HostKindIcon, MobileAgentIcon, OrcaHookTrustDrawer, PickerListDrawer, SmartWorkspaceSourceDrawer, View
- `src/components/NewWorktreeProjectTargetFields.tsx` — HostKindIcon, ListGroup, ListRow, SectionHeader, View
- `src/components/OrcaHookTrustDrawer.tsx` — BottomDrawer, Check, Pressable, Text, View
- `src/components/OrcaLogo.tsx` — Defs, G, Path, Svg
- `src/components/PickerListDrawer.tsx` — BottomDrawer, FlatList, ListRow, SheetHeader, View
- `src/components/PickerModal.tsx` — BottomDrawer, ListGroup, ListRow, MobileSearchField, PickerModalContent, Text, View
- `src/components/ProtocolBlockScreen.tsx` — Pressable, Text, View
- `src/components/RightDrawer.tsx` — Animated.ScrollView, Animated.View, GestureDetector, GestureHandlerRootView, MountedRightDrawer, Pressable, View
- `src/components/SheetPrimaryButton.tsx` — ActivityIndicator, Icon, Pressable, Text
- `src/components/SmartRailRow.tsx` — ListRow, MobileSwitch
- `src/components/SmartSourceModeIcon.tsx` — CaseSensitive, GitBranch, Sparkles, TaskProviderLogo
- `src/components/SmartWorkspaceAdvancedFields.tsx` — ListGroup, ListRow, MobileSwitch, Text, TextField, View
- `src/components/SmartWorkspaceSourceDrawer.tsx` — ActivityIndicator, BottomDrawer, FlatList, Pressable, SheetHeader, SmartSourceModeIcon, SmartWorkspaceSourceRow, Text, TextField, View
- `src/components/SmartWorkspaceSourceField.tsx` — CircleDot, ExternalLink, GitBranch, GitMerge, GitPullRequest, ListGroup, ListRow, Pressable, SectionHeader, SelectionIcon, TaskProviderLogo, TextField, View, X
- `src/components/SmartWorkspaceSourceRow.tsx` — GitBranch, ListRow, Sparkles, TaskProviderLogo, Text, View
- `src/components/StatusDot.tsx` — View
- `src/components/SwipeRowActions.tsx` — Animated.View, GestureDetector, Icon, Pressable, Text, View
- `src/components/TailClippedText.tsx` — Text, View
- `src/components/TaskProviderLogo.tsx` — Path, Svg
- `src/components/TerminalShortcutSettings.tsx` — CustomKeyModal, DragReorderList, ListRow, MobileSwitch, Pressable, SettingsEmptyRow, SettingsSection, ShortcutBarRow, SmartRailRow, Text, View, X
- `src/components/TextInputModal.tsx` — ActivityIndicator, BottomDrawer, NoticeIcon, Pressable, Text, TextField, TextInputSheetContent, View
- `src/components/TransientToast.tsx` — Text, View
- `src/components/VoiceModelList.tsx` — ActivityIndicator, Check, Download, ListGroup, ListRow, Pressable, Text, Trash2, View
- `src/components/WorkspaceDetailPlaceholder.tsx` — SquareTerminal, Text, View
- `src/components/WorktreeAgentChildDisclosure.tsx` — AgentStateDot, ChevronDown, ChevronRight, Pressable, Text, View
- `src/components/WorktreeAgentList.tsx` — Fragment, View, WorktreeAgentChildToggle, WorktreeAgentIdleGroupRow, WorktreeAgentRow, WorktreeAgentSummary
- `src/components/WorktreeAgentRow.tsx` — AgentStateDot, MobileAgentIcon, Pressable, Text, View
- `src/components/WorktreeAgentSummary.tsx` — AgentStateDot, ChevronDown, ChevronRight, MobileAgentIcon, Pressable, Text, View
- `src/components/WorktreeListRow.tsx` — AgentSpinner, Bell, ChevronDown, ChevronRight, GitBranch, HostKindIcon, MobileRepoIcon, PrStateIcon, Pressable, Text, View, WorktreeAgentList, WorktreeMetaGlyphs, WorktreeUnsavedName
- `src/components/WorktreeMetaGlyphs.tsx` — CalendarClock, CircleDot, GitMerge, SquareTerminal, StickyNote, TaskProviderLogo, Text, View
- `src/components/WorktreeUnsavedName.tsx` — PencilLine, Text, View
- `src/components/bottom-drawer/BottomDrawer.tsx` — Animated.View, KeyedBottomDrawer, View
- `src/components/bottom-drawer/bottom-drawer-modal-host.tsx` — BottomDrawerModalHostContext.Provider, Modal
- `src/components/bottom-drawer/keyed-bottom-drawer.tsx` — MountedBottomDrawer
- `src/components/bottom-drawer/mounted-bottom-drawer.tsx` — Animated.ScrollView, Animated.View, AutoHeight, GestureDetector, GestureHandlerRootView, Modal, Pressable, ScrollView, View
- `src/components/composer-input/ComposerSendButton.tsx` — ActivityIndicator, ArrowUp, Pressable
- `src/components/composer-input/FieldTextInput.tsx` — TextInput
- `src/components/composer-input/MessageBox.tsx` — ComposerSendButton, TextInput, View
- `src/components/composer-input/TextField.tsx` — Text, TextInput, View
- `src/components/detail-card-host.web.tsx` — DetailCardHostContext.Provider, span
- `src/components/list/ListGroup.tsx` — Check, ChevronDown, ChevronRight, DisclosureChevron, Fragment, Pressable, Text, View
- `src/components/mobile-markdown-blocks.tsx` — Fragment, MarkdownText, MermaidDiagram, MobileMarkdownCallout, MobileMarkdownCodeBlock, MobileMarkdownDetails, MobileMarkdownMath, NativeText, Pressable, ScrollView, Square, SquareCheck, View
- `src/components/mobile-markdown-image-paragraph.tsx` — Fragment, MarkdownText, View
- `src/components/mobile-markdown-inline.tsx` — Fragment, MarkdownText, MobileMarkdownMath
- `src/components/nav-bar/BottomSearchBar.tsx` — Animated.View, Defs, Icon, LeadingSlot, LinearGradient, MobileSearchField, Pressable, Rect, RoundButton, Stop, Svg, Text, View
- `src/components/nav-bar/FilterChip.tsx` — ChevronDown, Icon, Pressable, Text, View
- `src/components/nav-bar/NavBar.tsx` — Animated.View, ChevronDown, NavBarRefreshLine, Pressable, Text, View
- `src/components/nav-bar/NavBarButton.tsx` — Icon, Pressable
- `src/components/nav-bar/PageTitle.tsx` — Text, View
- `src/components/nav-bar/PullDownMenu.tsx` — Animated.View, Check, Icon, Modal, Pressable, ScrollView, Text, View
- `src/components/nav-bar/SheetHeader.tsx` — ActivityIndicator, NavBarButton, Pressable, Text, TextAction, View
- `src/components/pr-sidebar/MermaidDiagram.tsx` — ExpandedDiagram, Maximize2, MermaidFrame, MobileNativeChatImageViewer, Pressable, ScrollView, Text, View
- `src/components/pr-sidebar/MobileLinkPrForm.tsx` — ActivityIndicator, Pressable, Text, TextField, View
- `src/components/pr-sidebar/MobilePrTabSheets.tsx` — ConfirmModal, FieldTextInput, KeyedBottomDrawer, ListGroup, ListRow, PrCreateSheet, PrEditTextSheet, PrMergeSheet, PrReviewersSheet, PrTabConfirm, SheetHeader, SheetPrimaryButton, Text, View
- `src/components/pr-sidebar/PRCheckDetail.tsx` — ActivityIndicator, JobRow, ScrollView, Text, View
- `src/components/pr-sidebar/PRCommentCard.tsx` — Check, ConfirmModal, CornerDownRight, ExternalLink, Image, PRCommentComposer, Pencil, PrMarkdown, Pressable, Reactions, Text, Trash2, Undo2, View
- `src/components/pr-sidebar/PRCommentComposer.tsx` — MessageBox, Pressable, X
- `src/components/pr-sidebar/PRCommentsSection.tsx` — ActivityIndicator, Chevron, CommentGroupView, MobileMarkdown, PRCommentCard, PRCommentComposer, PRSection, Pressable, Text, View
- `src/components/pr-sidebar/PRSection.tsx` — Text, View
- `src/components/pr-sidebar/PrBasePicker.tsx` — ListGroup, ListRow, Search, SheetHeader, Text, TextField, View
- `src/components/pr-sidebar/PrCheckFailureRow.tsx` — ActivityIndicator, CircleX, MoreHorizontal, PRCheckDetailView, PrMiniButton, Pressable, PullDownMenu, Text, View
- `src/components/pr-sidebar/PrChecksScreen.tsx` — AgentLaunchNotice, CheckRow, ChecksSummary, ChevronDown, CircleCheck, DisclosureChevron, Icon, ListGroup, ListRow, NotReady, PrCheckFailureRow, ScreenFrame, ScrollView, SectionHeader, Text, View
- `src/components/pr-sidebar/PrCommentAvatar.tsx` — Text, View
- `src/components/pr-sidebar/PrCommentsScreen.tsx` — AgentLaunchNotice, MobileLoadErrorState, MobileSegmentedControl, MobileStateView, NavBarButton, NotReady, PRCommentComposer, PrThreadRow, PrThreadScreen, Pressable, PullDownMenu, ScreenFrame, ScrollView, SectionHeader, ShowMore, Text, View
- `src/components/pr-sidebar/PrCreateSheet.tsx` — ActivityIndicator, ChevronsUpDown, FieldTextInput, ListGroup, ListRow, MobileCommitFailurePanel, MobileSwitch, PrBasePicker, Pressable, SheetHeader, SheetPrimaryButton, Sparkles, Text, View
- `src/components/pr-sidebar/PrMarkdown.tsx` — MobileMarkdown, MobileMarkdownLinksContext.Provider
- `src/components/pr-sidebar/PrMiniButton.tsx` — ActivityIndicator, Icon, Pressable, Text
- `src/components/pr-sidebar/PrReviewersSheet.tsx` — ActivityIndicator, ListGroup, ListRow, Pressable, Search, SheetHeader, Text, TextField, View, X
- `src/components/pr-sidebar/PrTabBottomAction.tsx` — ActionButton, ActivityIndicator, AgentLaunchNotice, GitMerge, Icon, Pressable, Text, View
- `src/components/pr-sidebar/PrTabEmptyState.tsx` — GitPullRequest, Link2, ListGroup, ListRow, MobileLinkPrForm, Text, View
- `src/components/pr-sidebar/PrTabOverviewCards.tsx` — ActivityIndicator, AutoHeight, PrMarkdown, Pressable, Text, View
- `src/components/pr-sidebar/PrTabScreenView.tsx` — PrChecksScreen, PrCommentsScreen
- `src/components/pr-sidebar/PrTextLink.tsx` — ActivityIndicator, Pressable, Text
- `src/components/pr-sidebar/PrThreadCode.tsx` — Text, View
- `src/components/pr-sidebar/PrThreadRow.tsx` — ActivityIndicator, ConfirmModal, FileText, MoreHorizontal, PRCommentComposer, PrCommentAvatar, PrCommentFileLink, PrMarkdown, PrTextLink, PrThreadCode, Pressable, PullDownMenu, SeverityPill, Sparkles, Text, View
- `src/components/pr-sidebar/PrThreadScreen.tsx` — MobileStateView, PRCommentComposer, PrCommentAvatar, PrCommentFileLink, PrMarkdown, PrMiniButton, PrThreadCode, ScreenFrame, ScrollView, Text, View
- `src/components/pr-sidebar/pr-tab-screen-frame.tsx` — MobileLoadErrorState, MobileStateView, NavBar, NavBarButton, SafeAreaView, View
- `src/diagnostics/connection-diagnostics-screen.tsx` — ConnectionDiagnosticsView
- `src/diagnostics/connection-diagnostics-view.tsx` — Check, ConnectionLog, Copy, Pressable, Send, SettingsNavBar, Text, View
- `src/diagnostics/mobile-web-bundle-probe-row.tsx` — ActivityIndicator, ListGroup, ListRow, Package, Pressable, ProbeLine, ProbeResult, Text, View
- `src/diagnostics/mobile-web-shell-update-failure-row.tsx` — ListGroup, SectionHeader, Text, View
- `src/diagnostics/troubleshoot-view.tsx` — Activity, ActivityIndicator, AlertTriangle, CheckCircle2, ChevronDown, ChevronUp, ListGroup, ListRow, Pressable, ScrollText, ScrollView, SectionHeader, SettingsNavBar, StatusIcon, Text, View, XCircle, section.icon
- `src/dictation/MobileDictationLiveControls.tsx` — ActivityIndicator, CircleAlert, EngineIcon, Laptop, Mic, MobileDictationGrapes, MobileDictationTimer, Pressable, RotateCcw, Smartphone, Square, Text, View, X
- `src/dictation/MobileDictationMeter.tsx` — Text, View
- `src/dictation/MobileDictationPill.tsx` — CircleAlert, Laptop, MobileDictationCancelButton, MobileDictationGrapes, MobileDictationMicIcon, MobileDictationPill, MobileDictationRetryButton, MobileDictationStatusAnnouncement, MobileDictationTimer, MobileDictationToast, PillStatus, Pressable, Smartphone, Text, View
- `src/dictation/MobileDictationStatusAnnouncement.tsx` — Text, View
- `src/dictation/mobile-dictation-host.tsx` — MobileDictationClearanceContext.Provider, MobileDictationHostContext.Provider, MobileDictationPillContext.Provider
- `src/drafts/DocumentEditDraftOfferBar.tsx` — Pressable, Text, View
- `src/files/MobileFilePreviewScreen.tsx` — MobileChatAttachmentPreview, MobileFilePreviewBody, MobileFilePreviewDiscardModal, MobileFilePreviewDocument, MobileFilePreviewEditDraft, MobileMarkdownPreviewModeBar, NavBar, NavBarButton, SafeAreaView, View, WorkspaceFilePreviewScreen
- `src/files/attachments/AttachmentTextScrollView.tsx` — ScrollView, Text, View
- `src/files/attachments/MobileChatAttachmentPreview.tsx` — AttachmentTextScrollView, Pressable, SafeAreaView, Text, View
- `src/files/explorer/MobileFileExplorerPanel.tsx` — BottomSearchBar, FlatList, MobileFileExplorerRow, MobileLoadErrorState, MobileNoMatchState, MobileOfflineState, MobileStateView, NavBar, NavBarButton, SafeAreaView, View
- `src/files/explorer/mobile-file-explorer-row-actions-sheet.tsx` — ActionSheetModal, MobileFileExplorerRowActionsSheet
- `src/files/explorer/mobile-file-explorer-row.tsx` — ActivityIndicator, ChevronDown, ChevronRight, File, FileText, Folder, FolderPagingRow, ImageIcon, Pressable, Text, TreeRow, View
- `src/files/markdown/MobileFileMarkdownPreview.tsx` — MobileFilePreviewSourceText, MobileFilePreviewTruncatedNote, MobileMarkdown, MobileMarkdownPreviewImage, ScrollView
- `src/files/markdown/MobileMarkdownPreviewImage.tsx` — ActivityIndicator, Image, MobileLoadErrorState, MobileNativeChatImageViewer, Pressable, Text, View
- `src/files/markdown/MobileMarkdownPreviewModeBar.tsx` — MobileSegmentedControl, MobileSourceCopyButton, View
- `src/files/media/MobileFileMediaPreview.tsx` — ActivityIndicator, MediaDownload, MobileLoadErrorState, MobileMediaPlayback, MobilePdfPreview, Text, View
- `src/files/media/MobileMediaPlayback.tsx` — Text, View, audio, video
- `src/files/media/MobilePdfPreview.tsx` — ActivityIndicator, MobileLoadErrorState, MobileStateButton, Text, View
- `src/files/preview/MobileFilePreviewBody.tsx` — EditablePreviewSource, MobileFileMarkdownPreview, MobileFileMediaPreview, MobileFilePreviewEditableSource, MobileFilePreviewSourceText, MobileHtmlPreview, MobileLoadErrorState, MobileOfflineState, MobilePanZoomImage, MobileStateView, MobileSvgPreview, Text, View
- `src/files/preview/MobileFilePreviewDiscardModal.tsx` — ConfirmModal
- `src/files/preview/MobileFilePreviewEditableSource.tsx` — Text, TextInput, View
- `src/files/preview/MobileFilePreviewSourceText.tsx` — MobileFilePreviewTruncatedNote, MobileSyntaxSegments, ScrollView, Text
- `src/files/session-documents/MobileSessionFileReader.tsx` — Copy, DiffLineRow, FileReaderDocument, FlatList, MessageSquare, MobileFileMediaPreview, MobileHtmlPreview, MobileLoadErrorState, MobilePanZoomImage, MobileStateView, MobileSvgPreview, MobileSyntaxSegments, Pressable, ScrollView, Send, Text, View
- `src/files/session-documents/MobileSessionMarkdownReader.tsx` — ActivityIndicator, DocumentEditDraftOfferBar, MobileLoadErrorState, MobileRichMarkdownEditor, MobileStateView, Pressable, RefreshCw, Text, View
- `src/files/terminal-artifacts/MobileFilePreviewEditDraft.tsx` — DocumentEditDraftOfferBar, OfferBar
- `src/home/AppUpdateCard.tsx` — ArrowUpFromLine, Pressable, Text, View, X
- `src/home/MobileHomeActiveAgents.tsx` — MobileAgentListRows, MobileHomeSectionHeading
- `src/home/MobileHomeAppUpdateCard.tsx` — AppUpdateCard
- `src/home/MobileHomeDesktopRow.tsx` — ChevronRight, MachineGlyph, Pressable, Text, View
- `src/home/MobileHomeEmptyState.tsx` — Animated.View, Fragment, OrcaLogo, PairDesktopButton, Pressable, QrCode, ScrollView, Text, View
- `src/home/MobileHomeHostList.tsx` — Fragment, MobileHomeDesktopRow, MobileHomeHostRow, MobileHomeSectionHeading, Plus, Pressable, Text, View
- `src/home/MobileHomeScreen.tsx` — ActionSheetModal, ConfirmModal, GestureHandlerRootView, MobileHomeAgentsTab, MobileHomeAutomationsTab, MobileHomeFab, MobileHomeFeedTab, MobileHomeSettingsTab, MobileHomeTabBar, MobileHomeUsageTab, SafeAreaView, View
- `src/home/MobileHomeSectionHeading.tsx` — AsideIcon, Pressable, Text, View
- `src/home/MobileHomeStats.tsx` — MobileHomeSectionHeading, Text, View
- `src/home/MobileHomeTasksSection.tsx` — ChevronRight, HomeSection, Icon, PickerModal, Pressable, Text, View
- `src/home/tabs/MobileHomeAgentsDesktopFilter.tsx` — DesktopPill, MobileHomePill, Monitor, PickerModal, WifiOff
- `src/home/tabs/MobileHomeAgentsTab.tsx` — ActivityIndicator, AgentListSectionHeading, AgentsDesktopFilter, AgentsEmpty, Bot, ChevronRight, MobileAgentListRows, MobileHomeSectionHeading, MobileHomeTabHeader, MobileHomeTabHeaderButton, MobileSearchField, PullDownMenu, ScrollView, Text, View
- `src/home/tabs/MobileHomeAutomationRows.tsx` — AgentSpinner, ChevronRight, MobileSwitch, Pressable, Text, View
- `src/home/tabs/MobileHomeAutomationsTab.tsx` — ActivityIndicator, Animated.View, AutomationsEmpty, CalendarClock, Fragment, HostAutomations, MobileHomeAutomationRow, MobileHomeAutomationRunRow, MobileHomeHostPill, MobileHomeSectionHeading, MobileHomeTabHeader, ScrollView, Text, View
- `src/home/tabs/MobileHomeFab.tsx` — Animated.View, Plus, Pressable, Text, View
- `src/home/tabs/MobileHomeFeedTab.tsx` — MobileHomeActiveAgents, MobileHomeAppUpdateCard, MobileHomeEmptyState, MobileHomeHostList, MobileHomeTabHeader, MobileHomeTasksSection, ScrollView
- `src/home/tabs/MobileHomeHostPill.tsx` — Animated.View, ChevronDown, MobileHomePill, PickerModal, Pressable, Text, View
- `src/home/tabs/MobileHomeSettingsTab.tsx` — MobileHomeTabHeader, PendingCredentialCleanupCard, ScrollView, SettingsAppUpdateSection, SettingsMenuSections
- `src/home/tabs/MobileHomeTabBar.tsx` — Animated.View, Icon, Pressable, TabBadge, TabButton, Text, View
- `src/home/tabs/MobileHomeTabHeader.tsx` — Animated.View, Icon, Pressable, Text, View
- `src/home/tabs/MobileHomeUsageTab.tsx` — ActivityIndicator, Animated.View, ClaudeIcon, Gauge, HostUsage, MobileHomeHostPill, MobileHomeSectionHeading, MobileHomeStats, MobileHomeTabHeader, MobileHomeTabHeaderButton, OpenAIIcon, Pressable, ScrollView, Text, UsageCard, UsageEmpty, UsageMeter, View
- `src/host-screen/HostScreen.tsx` — HostScreenView
- `src/host-screen/host-bottom-bar.tsx` — BottomSearchBar
- `src/host-screen/host-screen-header.tsx` — HostConnectionStatus, NavBar, NavBarButton, NavBarSubtitleText, Pressable, PullDownMenu, StatusDot, Text
- `src/host-screen/host-screen-overlays.tsx` — ActionSheetContent, BottomDrawer, BottomDrawerModalHost, ConfirmModal, NewWorktreeModalController, OrcaHookTrustDrawer, Pressable, Text, View, WorkspaceDetailsSheet, WorkspaceOptionsSheet, WorkspaceRenameSheet
- `src/host-screen/host-screen-view.tsx` — HostBottomBar, HostConnectionStatus, HostScreenHeader, HostScreenOverlays, HostWorkspaceList, PageTitle, SafeAreaView, Text, View
- `src/host-screen/workspace-actions/workspace-details-sheet.tsx` — CircleDot, ExternalLink, GitMerge, LinkIcon, LinkRow, ListGroup, ListRow, OriginIcon, PrStateIcon, ReviewIcon, StickyNote, TaskProviderLogo, Text, View
- `src/host-screen/workspace-actions/workspace-rename-sheet.tsx` — TextInputSheetContent
- `src/host-screen/workspace-list/host-pending-creation-row.tsx` — ActivityIndicator, Text, View
- `src/host-screen/workspace-list/host-workspace-list.tsx` — ActivityIndicator, Animated.SectionList, AuthFailedBanner, ChevronRight, DisclosureChevron, FolderTree, HostDiagnosticsLink, HostOfflineBanner, HostPendingCreationRow, HostRouteNoticeBanner, HostSectionHeader, HostWorkspaceListReopen, HostWorkspaceListStates, MobileRepoIcon, Pin, Presence, Pressable, RefreshControl, Text, View, WorktreeListRow
- `src/host-screen/workspace-options/workspace-option-controls.tsx` — CardPropertyRows, CheckRow, ChoiceRows, ListGroup, MobileSearchField, MobileSegmentedControl, SectionHeader, SwitchRow, View
- `src/host-screen/workspace-options/workspace-option-rows.tsx` — CheckRow, ListGroup, ListRow, MobileSwitch, View
- `src/host-screen/workspace-options/workspace-options-sheet.tsx` — ActivityIndicator, BottomDrawer, DrillRow, ListGroup, NavBarButton, OptionPager, SectionHeader, SheetHeader, Text, View, WorkspaceOptionsContent, WorkspaceOptionsPending, control.Row, pageControl.Page
- `src/layout/side-rail.tsx` — SideRailContext.Provider, SideRailOutlet, View
- `src/layout/side-safe-area.tsx` — SafeAreaFrameContext.Provider, SafeAreaInsetsContext.Provider, SafeAreaProvider, View, WindowSafeAreaInsetsContext.Provider
- `src/mobile-web-shell/DownloadProgressBar.tsx` — Animated.View, View
- `src/mobile-web-shell/MobileWebShellScreen.tsx` — HostRouteNoticeBanner, OrcaMobileWebShellView, ShellDevFacts, ShellPageCover, ShellStateScreen, View
- `src/mobile-web-shell/PageRouteUnavailableScreen.tsx` — Pressable, Text, View
- `src/mobile-web-shell/ShellDevFacts.tsx` — Text, View
- `src/mobile-web-shell/ShellDownloadProgress.tsx` — DownloadProgressBar, ShellWaitingFrame
- `src/mobile-web-shell/ShellStateScreen.tsx` — Centered, Failed, PageRouteUnavailableScreen, Pressable, ProtocolBlockScreen, ShellDownloadProgress, ShellWaitingFrame, Text, View
- `src/mobile-web-shell/ShellWaitingFrame.tsx` — ActivityIndicator, Animated.View, ShellWaitingFrame, Text, View
- `src/mobile-web-shell/catch-all-page-route.tsx` — MobileWebShellScreen, PageRouteUnavailableScreen
- `src/mobile-web-shell/catch-all-page-route.web.tsx` — PageRouteUnavailableScreen
- `src/mobile-web-shell/terminal-direction-pad.tsx` — Pressable, Text, View
- `src/mobile-web-shell/terminal-key-rail.tsx` — ChevronDown, ChevronsRight, Ellipsis, Keyboard, Monitor, Pressable, ScrollView, Smartphone, TerminalDirectionPad, Text, View
- `src/mobile-web-shell/use-shell-key-rail.tsx` — TerminalKeyRail
- `src/navigation/host-stack.tsx` — Stack, Stack.Screen
- `src/navigation/workspace-route.tsx` — HostStatusGatesContext.Provider, PageRouteUnavailableScreen, Screen, WorkspaceExecutionHostContext.Provider, WorkspaceExecutionScope, WorkspaceRoute, WorkspaceServerContext.Provider
- `src/new-workspace/NewWorkspaceHostPicker.tsx` — PickerModal
- `src/new-workspace/NewWorkspaceLoadingDrawer.tsx` — ActivityIndicator, Animated.View, DownloadProgress, DownloadProgressBar, Pressable, Text, View
- `src/new-workspace/NewWorkspaceOverlay.tsx` — HostProtocolGate, MobileWebShellScreen, NewWorkspaceLoadingDrawer, OpenHostListInstead
- `src/new-workspace/NewWorkspacePageScreen.tsx` — NewWorktreeModalController
- `src/notifications/delivery-policy/NotificationDeliverySection.tsx` — ListRow, MobileSwitch, SettingsSection
- `src/onboarding/MobileOnboardingPage.tsx` — ActivityIndicator, ChoiceButton, MessageSquare, NotificationChoices, NotificationOnboardingPreview, Pressable, ScrollView, SessionViewChoices, Text, View
- `src/onboarding/NotificationOnboardingPreview.tsx` — Animated.View, OrcaLogo, SampleBanner, Text, View
- `src/pairing/pairing-visuals.tsx` — ActivityIndicator, Animated.View, Check, Text, View
- `src/query/orca-query-provider.tsx` — QueryClientProvider
- `src/session/MobileNativeChatForkProvider.tsx` — MobileNativeChatForkContext.Provider, MobileNativeChatForkedFromContext.Provider
- `src/session/MobileSessionActiveContent.tsx` — ActivityIndicator, Animated.View, FileReader, MarkdownReader, MobileAgentMessageProvider, MobileBrowserPane, MobileNativeChatDictationPreviewContext.Provider, MobileNativeChatForkProvider, MobileNativeChatOverlay, MobileSessionContentState, MountedTerminalPanes, Pressable, TerminalPaneView, Text, View
- `src/session/MobileSessionCommandDock.tsx` — KeyboardIcon, LiveInputField, MobileSessionAccessoryKeys, MobileTerminalInputActions, MobileTerminalLiveInputStatus, MobileTerminalMoreKeysSheet, TextField, View
- `src/session/MobileSessionContentRow.tsx` — AlertTriangle, MobileDictationLiveContext.Provider, MobileSessionActiveContent, MobileSessionCommandDock, Pressable, SessionDockColumn, Text, View, X
- `src/session/MobileSessionContentState.tsx` — MobileOfflineState, MobileStateView, OfflineUnderLastTabs
- `src/session/MobileSessionHeader.tsx` — MobileSessionHeaderMoreMenu, MobileSessionTabBar, MobileSessionTitleBlock, MobileSourceControlButton, NavBar, NavBarButton, NavBarRefreshLine, SafeAreaView, View
- `src/session/MobileSessionHeaderMoreMenu.tsx` — MobileAgentSessionHistoryIcon, PullDownMenu
- `src/session/MobileSessionRouteScreen.tsx` — ActivityIndicator, MobileSessionSurface, Text, View
- `src/session/MobileSessionSheets.tsx` — ActionSheetModal, ConfirmModal, CustomKeyModal, MobileAgentSessionTabActionSheet, MobileBrowserTabActionSheet, MobileDictationSetupSheet, MobileNewTabSheet, MobileSessionTerminalLayoutSheet, MobileTerminalRenameModal, QuickCommandsSheet, TextInputModal
- `src/session/MobileSessionSurface.tsx` — AnchoredDetailCardLayer, MobileSessionContentRow, MobileSessionHeader, MobileSessionSheets, MobileWorkerAutoCloseStrip, StatusDot, Text, View
- `src/session/MobileSessionTitleBlock.tsx` — DetailCardFocusBoundary, Fragment, Pressable, StatusDot, Text
- `src/session/MobileSourceControlButton.tsx` — NavBarButton
- `src/session/SessionDockColumn.tsx` — SessionDockPanelContent, Suspense, View
- `src/session/SessionDockPanelContent.tsx` — MobileFileExplorerPanel, MobileSourceControlRoutePanel
- `src/session/native-chat/MobileNativeChatFloatingControls.tsx` — Animated.View, ArrowDown, Pressable, View
- `src/session/native-chat/MobileNativeChatLowerRegion.web.tsx` — MobileNativeChatComposerPresenceContext, MobileNativeChatLowerGeometryContext, MobileNativeChatLowerRegionContext, ScrollView, View
- `src/session/native-chat/MobileNativeChatOverlay.tsx` — Activity, MobileMarkdownLinksContext.Provider, MobileNativeChatNoticeContext.Provider, MobileNativeChatOverlayContent, MobileNativeChatReplyRevealsContext.Provider, MobileNativeChatRootItemSourceContext.Provider, MobileNativeChatRowOverlaysHost, MobileNativeChatView, MobileNativeChatVisualContext.Provider, MobileQueuedMessageComposerContext.Provider, View
- `src/session/native-chat/MobileNativeChatSendErrorBanner.tsx` — Pressable, Text, View, X
- `src/session/native-chat/MobileNativeChatView.tsx` — GestureHandlerRootView, MobileNativeChatAwaitingInputRow, MobileNativeChatDisclosureContext.Provider, MobileNativeChatDraftComposer, MobileNativeChatEmptyStateView, MobileNativeChatFloatingControls, MobileNativeChatInputLockStatus, MobileNativeChatLiveLine, MobileNativeChatLowerRegion, MobileNativeChatOfflineQueue, MobileNativeChatOpenFileContext, MobileNativeChatPromptCard, MobileNativeChatPromptStrip, MobileNativeChatQueueAbovePromptContext, MobileNativeChatRowList, MobileNativeChatScrollRootsBoundary, MobileNativeChatSendErrorBanner, MobileStateView, View
- `src/session/native-chat/composer/MobileNativeChatComposer.tsx` — MobileNativeChatComposer, MobileNativeChatComposerAllocation, MobileNativeChatComposerAttachButton, MobileNativeChatComposerAttachments, MobileNativeChatComposerField, MobileNativeChatComposerMic, MobileNativeChatComposerPlanChip, MobileNativeChatComposerSendSlot, MobileNativeChatComposerSuggestions, MobileNativeChatDictationLeading, MobileNativeChatEditActions, MobileNativeChatSessionOptionPickers, View
- `src/session/native-chat/composer/MobileNativeChatComposerAllocation.web.tsx` — AllocationContext, View
- `src/session/native-chat/composer/MobileNativeChatComposerAttachButton.tsx` — ActionSheetModal, ActivityIndicator, Plus, Pressable
- `src/session/native-chat/composer/MobileNativeChatComposerAttachments.tsx` — ActivityIndicator, FileText, Image, ImageIcon, Pressable, RemoveBadge, ScrollView, Text, View, X
- `src/session/native-chat/composer/MobileNativeChatComposerField.web.tsx` — MobileNativeChatComposerFieldCopy, TextInput, View
- `src/session/native-chat/composer/MobileNativeChatComposerFieldCopy.tsx` — Text, TokenText, View
- `src/session/native-chat/composer/MobileNativeChatComposerMenu.tsx` — Icon, MenuRow, MobileNativeChatComposerMenu, MobileNativeChatComposerMenuLayer, MobileNativeChatComposerMenuReservation, Pressable, ScrollView, Text, View
- `src/session/native-chat/composer/MobileNativeChatComposerMenuLayer.web.tsx` — View
- `src/session/native-chat/composer/MobileNativeChatComposerMic.tsx` — MobileDictationMicIcon, MobileDictationRetryButton, MobileNativeChatControlSwap, Pressable
- `src/session/native-chat/composer/MobileNativeChatComposerModeChip.tsx` — Icon, Pressable, Text, X
- `src/session/native-chat/composer/MobileNativeChatComposerPlanChip.tsx` — MobileNativeChatComposerModeChip
- `src/session/native-chat/composer/MobileNativeChatComposerSendSlot.tsx` — MobileNativeChatControlSwap, MobileNativeChatSendButton, MobileNativeChatSteerPreviewButton, MobileNativeChatStopButton
- `src/session/native-chat/composer/MobileNativeChatContextUsageRing.tsx` — AnchoredDetailCardLayer, Circle, DetailCardFocusBoundary, MobileNativeChatContextUsageRing, Pressable, Svg, Text, View
- `src/session/native-chat/composer/MobileNativeChatDictationLeading.tsx` — MobileDictationCancelButton, MobileDictationFailureNote, MobileDictationFinishingNote, MobileDictationRecordingMeter, View
- `src/session/native-chat/composer/MobileNativeChatEditActions.tsx` — Pressable, Text
- `src/session/native-chat/composer/MobileNativeChatInputLockStatus.tsx` — Pressable, Text, View
- `src/session/native-chat/composer/MobileNativeChatSendButton.tsx` — ActionSheetModal, ComposerSendButton, Icon, Pressable
- `src/session/native-chat/input/MobileNativeChatAsk.tsx` — Check, MessageBox, MobileNativeChatAskOptionRow, MobileNativeChatCardHeaderAction, MobileNativeChatOptionPreviewSheet, MobileNativeChatQuestionText, Pressable, ScrollView, Text, View
- `src/session/native-chat/input/MobileNativeChatAskOptionRow.tsx` — Check, MobileNativeChatOptionThumbnail, Pressable, Text, View
- `src/session/native-chat/input/MobileNativeChatBackgroundTasks.tsx` — Activity, AgentStateDot, BackgroundTaskKindIcon, Bot, ChevronDown, CircleHelp, Pressable, ScrollView, SquareTerminal, TaskRow, Text, View, Workflow
- `src/session/native-chat/input/MobileNativeChatCardHeaderAction.tsx` — Icon, Pressable
- `src/session/native-chat/input/MobileNativeChatDockedTaskList.tsx` — ChevronUp, ListChecks, MobileNativeChatChecklist, Pressable, ScrollView, Text, View
- `src/session/native-chat/input/MobileNativeChatOfflineQueue.tsx` — MobileNativeChatQueuedMessages
- `src/session/native-chat/input/MobileNativeChatOptionPreviewSheet.tsx` — KeyedBottomDrawer, Pressable, ReadablePreview, ScrollView, SheetHeader, SheetPrimaryButton, Text, View
- `src/session/native-chat/input/MobileNativeChatOptionThumbnail.tsx` — MeasuredPreviewThumbnail, MobileEdgeFade, Pressable, Text, View
- `src/session/native-chat/input/MobileNativeChatPermission.tsx` — MobileNativeChatCardHeaderAction, MobileNativeChatMarkdown, MobileNativeChatPermissionContext, MobileNativeChatPermissionDetail, Pressable, ScrollView, ShieldQuestion, Text, View
- `src/session/native-chat/input/MobileNativeChatPromptCard.tsx` — Animated.View, MobileNativeChatAsk, MobileNativeChatPermission, MobileNativeChatQuestion, MobileNativeChatStructuredAsk, Pressable, PromptCardSlot, Text, View
- `src/session/native-chat/input/MobileNativeChatPromptStrip.tsx` — ChevronUp, Pressable, Text, View
- `src/session/native-chat/input/MobileNativeChatQuestion.tsx` — Check, CircleHelp, MessageBox, MobileNativeChatCardHeaderAction, MobileNativeChatQuestionText, Pressable, ScrollView, Text, View
- `src/session/native-chat/input/MobileNativeChatQueueFrame.tsx` — View
- `src/session/native-chat/input/MobileNativeChatQueuedMessages.tsx` — ActionSheetModal, ChevronDown, ChevronUp, DragReorderList, MobileNativeChatQueueFrame, MobileQueuedMessageCardRow, MobileQueuedMessageScrollList, Play, Pressable, Text, Trash2, View
- `src/session/native-chat/input/MobileNativeChatSessionOptionPickers.tsx` — BottomDrawer, Icon, ListGroup, LockKeyhole, ModelPill, NavBarButton, OptionRows, SectionHeader, SessionOptionCaption, SessionOptionSegments, SessionOptionSummaryRow, SheetHeader, Text, View
- `src/session/native-chat/input/MobileNativeChatSessionOptionRows.tsx` — ListRow, MobileSwitch, PillIcon, Pressable, Text, TintedValue, View
- `src/session/native-chat/input/MobileNativeChatSessionOptionSegments.tsx` — MobileSegmentedControl, SectionHeader, SessionOptionCaption, View
- `src/session/native-chat/input/MobileNativeChatStopButton.tsx` — ActivityIndicator, MobileNativeChatControlSwap, Pressable, Square
- `src/session/native-chat/input/MobileNativeChatStructuredAsk.tsx` — MobileNativeChatAsk
- `src/session/native-chat/input/MobileNativeChatTaskList.tsx` — ChevronRight, FullList, Icon, ListChecks, MobileNativeChatChecklist, OpenedChecklist, Pressable, TaskRow, Text, View
- `src/session/native-chat/input/MobileNativeChatThreadGoalStrip.tsx` — ChevronDown, Goal, GoalAction, Pause, Play, Pressable, ScrollView, Text, Trash2, View
- `src/session/native-chat/input/MobileQueuedCardText.tsx` — Pressable, Text, View
- `src/session/native-chat/input/MobileQueuedMessageCardRow.tsx` — AlertCircle, CornerDownRight, ImageIcon, MobileAgentMessageSenders, MobileQueuedCardText, MobileQueuedMessageComposer, MoreHorizontal, Pressable, QueuedMessageCardContents, Send, Text, View
- `src/session/native-chat/input/MobileQueuedMessageComposer.tsx` — MobileAgentMessageSenders, MobileNativeChatComposer, Text, View
- `src/session/native-chat/input/MobileQueuedMessageScrollList.tsx` — Animated.ScrollView, MobileEdgeFade, View
- `src/session/native-chat/input/mobile-native-chat-control-swap.tsx` — Animated.View, View
- `src/session/native-chat/transcript/MobileAgentMessageProvider.tsx` — MobileAgentMessageContext.Provider, MobileAgentMessageOpenContext.Provider
- `src/session/native-chat/transcript/MobileAgentMessageSenders.tsx` — ArrowUpRight, Fragment, Pressable, SenderName, Text, View
- `src/session/native-chat/transcript/MobileLinkActionSheet.tsx` — ActionSheetModal
- `src/session/native-chat/transcript/MobileNativeChatAgentMessageCard.tsx` — AgentStateDot, Bot, Chevron, MobileAgentIcon, MobileAgentMessageSenders, MobileNativeChatMarkdown, MobileNativeChatMessageTime, MobileSourceCopyButton, Pressable, SenderAvatar, Text, View
- `src/session/native-chat/transcript/MobileNativeChatAwaitingInputRow.tsx` — MessageSquareMore, MobileNativeChatQuestionText, Text, View
- `src/session/native-chat/transcript/MobileNativeChatBackgroundTaskRun.tsx` — AgentStateDot, BackgroundTaskKindIcon, Text, View
- `src/session/native-chat/transcript/MobileNativeChatDiffCard.tsx` — ChevronRight, DiffCardRows, DiffRow, Icon, MobileNativeChatOwnedScrollView, MobileSourceCopyButton, Pressable, Text, VerbIcon, View
- `src/session/native-chat/transcript/MobileNativeChatEmptyStateView.tsx` — Text, View
- `src/session/native-chat/transcript/MobileNativeChatFullContent.tsx` — MobileNativeChatOwnedScrollView, Pressable, ShowFull, Text, View
- `src/session/native-chat/transcript/MobileNativeChatInterruptedContinueButton.tsx` — MobileNativeChatNoticePill
- `src/session/native-chat/transcript/MobileNativeChatLiveLine.tsx` — ChevronRight, LoadingSpinner, MobileNativeChatReasoningBody, Pressable, Text, View
- `src/session/native-chat/transcript/MobileNativeChatLongPressContent.tsx` — Pressable, View
- `src/session/native-chat/transcript/MobileNativeChatMarkdown.tsx` — MobileMarkdown
- `src/session/native-chat/transcript/MobileNativeChatMessage.tsx` — Content, MobileNativeChatAgentMessageCard, MobileNativeChatAwaitingInputRow, MobileNativeChatBackgroundTaskRun, MobileNativeChatGroupedThought, MobileNativeChatMessageMeta, MobileNativeChatNoticeRow, MobileNativeChatPacedProse, MobileNativeChatPullRequestEventLine, MobileNativeChatReasoningRow, MobileNativeChatResolutionReceipt, MobileNativeChatSubagentGroup, MobileNativeChatTurnStatus, MobileNativeChatUserMessageFold, Prose, Text, TextAttachmentActions, ToolRun, View
- `src/session/native-chat/transcript/MobileNativeChatMessageActionsSheet.tsx` — ActionSheetContent, BottomDrawer, Modal, Pressable, ScrollView, SelectTextScreen, Text, View, X
- `src/session/native-chat/transcript/MobileNativeChatMessageMeta.tsx` — MobileNativeChatMessageTime, MobileNativeChatResponseActions, MobileNativeChatRewindButton, View
- `src/session/native-chat/transcript/MobileNativeChatMessageTime.tsx` — Text
- `src/session/native-chat/transcript/MobileNativeChatNoticeDisclosure.tsx` — ChevronRight, Pressable, Text, View
- `src/session/native-chat/transcript/MobileNativeChatNoticePill.tsx` — Icon, Pressable, Text
- `src/session/native-chat/transcript/MobileNativeChatNoticeRow.tsx` — Icon, MobileNativeChatInterruptedContinueButton, MobileNativeChatMarkdown, MobileNativeChatNoticeDisclosure, MobileNativeChatOwnedScrollView, MobileNativeChatUsageLimitStatus, Pressable, Text, View
- `src/session/native-chat/transcript/MobileNativeChatOwnedScrollView.tsx` — RootAddress.Provider, ScrollView
- `src/session/native-chat/transcript/MobileNativeChatPageList.tsx` — Fragment, MobileNativeChatGrowthHoldContext.Provider, MobileNativeChatPageScroll, div
- `src/session/native-chat/transcript/MobileNativeChatPageScroll.tsx` — ScrollView, View
- `src/session/native-chat/transcript/MobileNativeChatProse.tsx` — Image, ImageAttachment, MobileNativeChatMarkdown, MobileNativeChatMarkdownRowKeyContext.Provider, Pressable, Text, View
- `src/session/native-chat/transcript/MobileNativeChatPullRequestEventLine.tsx` — Icon, Text, View
- `src/session/native-chat/transcript/MobileNativeChatQuestionText.tsx` — MobileNativeChatMarkdown, View
- `src/session/native-chat/transcript/MobileNativeChatReasoningRow.tsx` — Brain, ChevronRight, MobileNativeChatLongPressContent, MobileNativeChatMarkdown, MobileNativeChatOwnedScrollView, MobileNativeChatReasoningBody, MobileNativeChatReasoningRow, Pressable, Text, View
- `src/session/native-chat/transcript/MobileNativeChatResolutionReceipt.tsx` — MobileNativeChatAwaitingInputRow, MobileNativeChatMessageTime, MobileNativeChatQuestionText, Text, View
- `src/session/native-chat/transcript/MobileNativeChatResponseActions.tsx` — ForkButton, GitFork, MobileSourceCopyButton, Pressable, View
- `src/session/native-chat/transcript/MobileNativeChatRewindButton.tsx` — Pressable, Undo2
- `src/session/native-chat/transcript/MobileNativeChatRewindDialog.tsx` — ConfirmModal
- `src/session/native-chat/transcript/MobileNativeChatRowItemSource.tsx` — MobileNativeChatItemSourceContext.Provider, MobileNativeChatMessage, MobileNativeChatRowContent, TurnDiffOutcome
- `src/session/native-chat/transcript/MobileNativeChatRowList.tsx` — FlatList, MobileNativeChatPageList, MobileNativeChatPinchZoom
- `src/session/native-chat/transcript/MobileNativeChatRowOverlaysHost.tsx` — MarkdownLinkHandlerContext.Provider, MobileLinkActionSheet, MobileNativeChatImageViewer, MobileNativeChatMessageActionsSheet, MobileNativeChatRewindContext.Provider, MobileNativeChatRewindDialog, MobileNativeChatRowOverlaysContext.Provider
- `src/session/native-chat/transcript/MobileNativeChatSubagentGroup.tsx` — Bot, ChevronRight, Elapsed, Pressable, Text, View
- `src/session/native-chat/transcript/MobileNativeChatTextAttachmentActions.tsx` — Text
- `src/session/native-chat/transcript/MobileNativeChatToolIcon.tsx` — Glyph
- `src/session/native-chat/transcript/MobileNativeChatToolLine.tsx` — ChevronRight, DiffView, MobileNativeChatOwnedScrollView, MobileNativeChatToolGlyph, MobileNativeChatToolOutput, MobileSourceCopyButton, MobileSyntaxSegments, Pressable, SearchResults, Text, ToolLineDetail, View
- `src/session/native-chat/transcript/MobileNativeChatToolRun.tsx` — Animated.Text, Check, MobileNativeChatDiffCard, MobileNativeChatOpenedDetailContext.Provider, MobileNativeChatOwnedScrollView, MobileNativeChatTaskList, MobileNativeChatToolGlyph, MobileNativeChatToolLine, Pressable, PulsingText, Text, ToolRunMemberList, ToolRunMembers, View
- `src/session/native-chat/transcript/MobileNativeChatTurnDiffRollup.tsx` — ChevronRight, FileRow, MobileNativeChatDiffCard, Pressable, Text, View
- `src/session/native-chat/transcript/MobileNativeChatTurnStatus.tsx` — ChevronRight, Pressable, Text, View
- `src/session/native-chat/transcript/MobileNativeChatUsageLimitStatus.tsx` — MobileNativeChatNoticePill, Text, View
- `src/session/native-chat/transcript/MobileNativeChatUserMessageFold.tsx` — Pressable, Text, View
- `src/session/native-chat/transcript/mobile-native-chat-list-footer.tsx` — Fragment
- `src/session/native-chat/transcript/mobile-native-chat-list-header.tsx` — ActivityIndicator, Animated.View, FadeIn, Pressable, Text, View
- `src/session/native-chat/transcript/mobile-native-chat-pinch-zoom.tsx` — GestureDetector
- `src/session/native-chat/transcript/use-mobile-native-chat-row-renderer.tsx` — MobileNativeChatRow
- `src/session/native-chat/visual/MobileImageViewerActions.tsx` — ActionSheetModal, Pressable, Share, TransientToast
- `src/session/native-chat/visual/MobileNativeChatImageViewer.tsx` — Actions, Animated.View, MobilePanZoomImage, Modal, Pressable, X
- `src/session/native-chat/visual/MobileNativeChatVisual.tsx` — ActivityIndicator, Maximize2, MobileNativeChatVisual, MobileNativeChatVisualFrame, MobileNativeChatVisualFullscreen, MobileNativeChatVisualPlaceholder, Modal, Pressable, Text, View, X
- `src/session/native-chat/visual/MobileNativeChatVisualFrame.tsx` — View, iframe
- `src/session/tabs/MobileAgentSessionTabActionSheet.tsx` — ActionSheetModal
- `src/session/tabs/MobileBrowserTabActionSheet.tsx` — ActionSheetModal
- `src/session/tabs/MobileNewTabSheet.tsx` — ActionSheetContent, BottomDrawer, FileText, Globe, ListGroup, ListRow, MobileAgentIcon, Pressable, SectionHeader, SheetHeader, SquareTerminal, Text, View
- `src/session/tabs/MobileSessionTabBar.tsx` — AgentStateDot, Animated.View, ArrivalFade, ArrivingTabBarDot, GestureDetector, MobileSessionTabMark, MobileSessionTabsSheet, Plus, Pressable, TabBarDot, Text, View
- `src/session/tabs/MobileSessionTabMark.tsx` — Icon, MobileAgentIcon
- `src/session/tabs/MobileSessionTabsSheet.tsx` — AgentStateDot, BottomDrawer, Fragment, ListGroup, MobileSessionTabMark, MobileSessionTabsSheetContent, NavBarButton, Pressable, SectionHeader, SheetHeader, SwipeRowActions, Text, View
- `src/session/tabs/MobileWorkerAutoCloseStrip.tsx` — HostRouteNoticeBanner, Presence
- `src/settings/about-screen.tsx` — GithubIcon, Globe, ListRow, OrcaLogo, Path, SettingsScreen, SettingsSection, Svg, Text, View, XIcon
- `src/settings/app-update-settings-rows.tsx` — ListRow, SettingsSection, Text
- `src/settings/appearance-settings-screen.tsx` — ListRow, SettingsScreen, SettingsSection, UiLanguageSettingsSection
- `src/settings/browser-settings-screen.tsx` — Globe, ListRow, PickerModal, SettingsNote, SettingsScreen, SettingsSection
- `src/settings/mobile-settings-menu.tsx` — Icon, ListRow, SettingsSection
- `src/settings/native-chat-settings-screen.tsx` — ListRow, MobileSwitch, SettingsScreen, SettingsSection
- `src/settings/native-notification-delivery-settings.tsx` — NotificationDeliverySection, SettingsNote
- `src/settings/notification-display-test.tsx` — ListRow, SettingsNote, SettingsSection
- `src/settings/notification-settings-screen.tsx` — ListRow, MobileSwitch, SettingsNote, SettingsScreen, SettingsSection
- `src/settings/pending-credential-cleanup-card.tsx` — ActivityIndicator, KeyRound, Pressable, SettingsSection, Text, View
- `src/settings/settings-app-update-section.tsx` — AppUpdateSettingsRows
- `src/settings/settings-menu-screen.tsx` — MobileSettingsSection, SettingsMenuSections, SettingsScreen
- `src/settings/settings-screen-frame.tsx` — ListGroup, NavBar, NavBarButton, SafeAreaView, ScrollView, SectionHeader, SettingsNavBar, SettingsNote, Text, View
- `src/settings/ui-language-settings-section.tsx` — ListRow, SettingsSection
- `src/settings/voice-settings-screen.tsx` — BottomDrawer, ListRow, MobileSegmentedControl, MobileStateView, MobileSwitch, SettingsNote, SettingsScreen, SettingsSection, Text, VoiceModelList, VoiceTranscribeOnSection
- `src/settings/voice-transcribe-on-section.tsx` — ListRow, SettingsSection
- `src/source-control/MobileSourceControlPanel.tsx` — MobileLoadErrorState, MobileOfflineState, MobilePrTabBody, MobilePrTabSheets, MobileSegmentedControl, MobileSourceControlContent, MobileSourceControlHeader, MobileSourceControlHubCommitsPage, MobileSourceControlModals, MobileSourceControlNextStepBar, MobileStateView, OptionPager, PrTabScreenView, SafeAreaView, View
- `src/source-control/MobileSourceControlRoutePanel.tsx` — MobileFolderSourceControlPanel, MobileSourceControlPanel
- `src/source-control/changes/MobileSourceControlChangesHeader.tsx` — ChevronDown, ChevronRight, GitBranch, GitCommitHorizontal, Pressable, Text, TriangleAlert, View
- `src/source-control/changes/MobileSourceControlContent.tsx` — ActivityIndicator, Animated.View, CleanStateFade, MobileCommitFailurePanel, MobileSourceControlChangesHeader, MobileSourceControlFileList, Text, View
- `src/source-control/changes/MobileSourceControlFileList.tsx` — ActivityIndicator, BranchCompareState, BranchFileRow, ChangeFileRow, Pressable, SectionHeader, SectionList, Text, View
- `src/source-control/changes/MobileSourceControlFileRows.tsx` — ActivityIndicator, FileRowBody, Minus, MobileDiffLineStats, Plus, Pressable, SwipeRowActions, Text, View
- `src/source-control/commits/MobileCommitFailurePanel.tsx` — ActivityIndicator, AgentLaunchNotice, Chevron, Pressable, Sparkles, Text, View
- `src/source-control/folder/MobileFolderEmptyState.tsx` — ActivityIndicator, FolderGit2, FolderRepoCreateCard, ListGroup, ListRow, MobileStateView, Plus, ScrollView, SectionHeader, View
- `src/source-control/folder/MobileFolderSourceControlPanel.tsx` — ListGroup, MobileFolderEmptyState, MobileFolderWorktreeRow, MobileLoadErrorState, MobileOfflineState, MobileSourceControlHeader, MobileSourceControlPanel, MobileStateView, OptionPager, SafeAreaView, ScrollView, View, WorkspaceExecutionScope
- `src/source-control/folder/MobileFolderWorktreeRow.tsx` — ListRow
- `src/source-control/folder/use-folder-create-flow.tsx` — ConfirmModal, NewWorktreeModalController, TransientToast
- `src/source-control/history/MobileCommitDetailPage.tsx` — ActivityIndicator, CommitFileRow, CommitLineStat, ListGroup, ListRow, NavBar, NavBarButton, Pressable, PullDownMenu, ScrollView, SectionHeader, Text, View
- `src/source-control/history/MobileCommitListRows.tsx` — ActivityIndicator, ChevronRight, CommitLineStat, Pressable, SectionHeader, Text, Timeline, View
- `src/source-control/history/MobileCommitsScreen.tsx` — MobileCommitDetailPage, MobileCommitPageHost, MobileGitHistoryList, OptionPager, SafeAreaView
- `src/source-control/history/MobileGitHistoryList.tsx` — ActivityIndicator, BranchBaseRow, CommitsSectionHeader, FlatList, MobileLoadErrorState, MobileOfflineState, MobileStateView, NavBar, NavBarButton, Pressable, RefreshControl, Text, View
- `src/source-control/panel/MobileSourceControlHeader.tsx` — NavBar, NavBarButton, PullDownMenu
- `src/source-control/panel/MobileSourceControlHubCommitsPage.tsx` — MobileCommitsScreen
- `src/source-control/panel/MobileSourceControlModals.tsx` — ConfirmModal, PickerModal
- `src/source-control/panel/MobileSourceControlNextStepBar.tsx` — ActivityIndicator, AgentLaunchNotice, BarButton, CommitComposer, ConfirmModal, GitMerge, Icon, MessageBox, Pressable, Sparkles, Text, View
- `src/source-control/review/MobileDiffReviewRouteScreen.tsx` — MobileDiffReviewScreenView
- `src/source-control/review/MobileSessionDiffLineRow.tsx` — MessageBox, MessageSquare, MobileSyntaxSegments, Plus, Pressable, Text, View, X
- `src/tasks/MobileTasksLegacySurface.tsx` — ActionSheetModal, AlertTriangle, BottomDrawerModalHost, ConfirmModal, MobileTasksItemDetailDrawer, MobileTasksOfflineBanner, Pressable, SafeAreaView, Text, View
- `src/tasks/hosted-review/MobileTasksPrFileDiff.tsx` — MessageBox, MobileSyntaxSegments, Text, View
- `src/tasks/item-detail/mobile-tasks-item-actions.tsx` — NavBarButton, Plus, Pressable, Text, View
- `src/tasks/item-detail/mobile-tasks-item-comments.tsx` — Text, View
- `src/tasks/item-detail/mobile-tasks-item-detail-content.tsx` — ExternalLink, ListGroup, ListRow, MobileLoadErrorState, MobileStateView, Pressable, SectionHeader, Text, TextField, View
- `src/tasks/item-detail/mobile-tasks-item-detail-drawer.tsx` — ActionSheetModal, BottomDrawer, Pressable, RefreshCw, TaskProviderLogo, Text, View
- `src/tasks/item-detail/mobile-tasks-item-field-editors.tsx` — ActivityIndicator, Check, Pressable, SectionHeader, Text, TextField, View
- `src/tasks/item-detail/mobile-tasks-item-review-sections.tsx` — ActivityIndicator, Check, GitHubPrFileDiff, MessageBox, MobileMarkdown, Pressable, SectionHeader, Text, TextField, View
- `src/tasks/item-detail/use-mobile-tasks-detail-comment-renderers.tsx` — MessageBox, MobileMarkdown, Pressable, Text, View
- `src/tasks/project-board/mobile-tasks-github-project-list.tsx` — ChevronDown, FlatList, MobileLoadErrorState, MobileNoMatchState, MobileStateView, Pressable, TaskProviderLogo, Text, View
- `src/tasks/project-board/mobile-tasks-github-project-pickers.tsx` — ActivityIndicator, AlertTriangle, BottomDrawer, ListGroup, ListRow, PickerModal, Pressable, Search, SectionHeader, Text, TextField, View
- `src/tasks/project-board/mobile-tasks-project-comments.tsx` — MessageBox, MobileMarkdown, Pressable, Text, View, X
- `src/tasks/project-board/mobile-tasks-project-detail-content.tsx` — ActivityIndicator, Text, View
- `src/tasks/project-board/mobile-tasks-project-detail-drawer.tsx` — ActivityIndicator, BottomDrawer, Copy, ExternalLink, GitBranch, ListGroup, ListRow, Plus, Pressable, RefreshCw, TaskProviderLogo, Text, View, X
- `src/tasks/project-board/mobile-tasks-project-field-editors.tsx` — Check, Pressable, Text, TextField, View
- `src/tasks/project-board/mobile-tasks-project-metadata-editors.tsx` — ActivityIndicator, Check, MessageBox, MobileMarkdown, Pressable, Text, TextField, View
- `src/tasks/project-board/mobile-tasks-project-review-panels.tsx` — ActivityIndicator, Check, ExternalLink, GitHubPrFileDiff, Pressable, Text, TextField, View
- `src/tasks/project-board/use-mobile-tasks-picker-projection.tsx` — View
- `src/tasks/project-board/use-mobile-tasks-workspace-create-projection.tsx` — MobileAgentIcon
- `src/tasks/providers/jira/mobile-tasks-jira-view-controls.tsx` — FilterChip
- `src/tasks/providers/linear/mobile-tasks-linear-list.tsx` — ChevronDown, FlatList, Pressable, ScrollView, TaskProviderLogo, Text, View
- `src/tasks/providers/linear/mobile-tasks-linear-view-controls.tsx` — FilterChip
- `src/tasks/screen/mobile-tasks-filter-pickers.tsx` — ActivityIndicator, BottomDrawer, ListGroup, ListRow, PickerModal, Text, View
- `src/tasks/screen/mobile-tasks-filter-sheet.tsx` — BottomDrawer, ListGroup, ListRow, Section, SectionHeader, SheetHeader, Text, View
- `src/tasks/screen/mobile-tasks-github-view-controls.tsx` — FilterChip
- `src/tasks/screen/mobile-tasks-host-subtitle.tsx` — Text
- `src/tasks/screen/mobile-tasks-provider-controls.tsx` — FilterChip, ScrollView
- `src/tasks/screen/mobile-tasks-provider-pickers.tsx` — BottomDrawer, ListGroup, ListRow, PickerModal, Pressable, ScrollView, Text, View
- `src/tasks/screen/mobile-tasks-screen-chrome.tsx` — MobileSegmentedControl, MobileTasksHostSubtitle, MobileTasksStatusBar, NavBar, NavBarButton, StatusDot, View
- `src/tasks/screen/mobile-tasks-search-control.tsx` — BottomSearchBar
- `src/tasks/task-list/mobile-tasks-list-display-pickers.tsx` — BottomDrawer, ListGroup, ListRow, Text, View
- `src/tasks/task-list/mobile-tasks-list-state.tsx` — HostOfflineBanner, MobileLoadErrorState, MobileNoMatchState, MobileOfflineState, MobileStateView, MobileTasksOfflineState, View
- `src/tasks/task-list/mobile-tasks-list-surface.tsx` — JiraConnectPrompt, MobileStateView, TaskProviderLogo
- `src/tasks/task-list/mobile-tasks-options.tsx` — TaskProviderLogo
- `src/tasks/task-list/mobile-tasks-provider-item-list.tsx` — ActivityIndicator, ChevronLeft, ChevronRight, ExternalLink, FlatList, GitBranch, Pressable, Text, View, glyph.icon
- `src/tasks/workspace-creation/mobile-tasks-create-drawers.tsx` — ActivityIndicator, BottomDrawer, ChevronDown, ExternalLink, Lock, MessageBox, PickerModal, Pressable, TaskProviderLogo, Text, TextField, View
- `src/tasks/workspace-creation/mobile-tasks-workspace-create-drawer.tsx` — AutoHeight, BottomDrawer, ListGroup, ListRow, MobileAgentIcon, MobileWorkspaceNameInput, NewWorkspaceSshConnectionField, PickerModal, SectionHeader, SheetHeader, SheetPrimaryButton, Text, View
- `src/tasks/workspace-creation/mobile-tasks-workspace-option-pickers.tsx` — ActivityIndicator, BottomDrawer, ListGroup, ListRow, Pencil, Pressable, Search, Text, TextField, View
- `src/tasks/workspace-creation/mobile-tasks-workspace-trust-drawers.tsx` — ActivityIndicator, BottomDrawer, Check, ListGroup, ListRow, MessageBox, Pressable, Text, TextField, View, X
- `src/terminal/quick-commands/QuickCommandEditorForm.tsx` — ActionToggle, ChevronDown, ChevronRight, MessageBox, MobileAgentIcon, MobileSwitch, Pressable, Text, TextField, View
- `src/terminal/quick-commands/QuickCommandRow.tsx` — Check, Copy, MobileAgentIcon, Pencil, Play, Pressable, Text, Trash2, View
- `src/terminal/quick-commands/QuickCommandsList.tsx` — ActivityIndicator, Check, MobileAgentIcon, Plus, Pressable, QuickCommandGroup, QuickCommandRow, Search, Text, TextField, View
- `src/terminal/quick-commands/QuickCommandsSheet.tsx` — BottomDrawer, ChevronLeft, Pressable, QuickCommandAgentPicker, QuickCommandEditorForm, QuickCommandsList, Text, View
- `src/terminal/session/accessory-keys/MobileSessionAccessoryKeys.tsx` — ChevronDown, KeyboardIcon, MobileSessionLatchKeys, MobileSessionSmartKeys, Monitor, Plus, Pressable, ScrollView, Smartphone, Text, View
- `src/terminal/session/accessory-keys/MobileSessionLatchKeys.tsx` — Pressable, Text
- `src/terminal/session/accessory-keys/MobileSessionSmartKeys.tsx` — MobileAgentIcon, Pressable, Text, View
- `src/terminal/session/accessory-keys/MobileTerminalMoreKeysSheet.tsx` — TerminalMoreKeysSheet
- `src/terminal/session/accessory-keys/TerminalMoreKeysSheet.tsx` — BottomDrawer, Icon, Plus, Pressable, SectionHeader, Text, View
- `src/terminal/session/input-controls/MobileTerminalInputActions.tsx` — ActivityIndicator, ImagePlus, MobileDictationMicIcon, MobileDictationRetryButton, MobileTerminalAttachButton, MobileTerminalDictationButton, Pressable
- `src/terminal/session/input-controls/MobileTerminalLiveInputStatus.tsx` — MobileDictationCancelButton, MobileDictationFailureNote, MobileDictationFinishingNote, MobileDictationRecordingMeter, TailClippedText, Text, View
- `src/terminal/session/pane-layout/MobileSessionTerminalLayoutSheet.tsx` — ActionSheetModal
- `src/terminal/session/pane-layout/TerminalPaneView.tsx` — Pressable, TerminalWebView, Text, View
- `src/terminal/session/tab-actions/MobileTerminalRenameModal.tsx` — BottomDrawer, TextInputSheetContent
- `src/terminal/webview/TerminalWebView.web.tsx` — TerminalWebViewEngineErrorOverlay, View
- `src/terminal/webview/terminal-webview-engine-error-state.tsx` — Pressable, RefreshCw, Text, View
- `src/transport/connection/client-context.tsx` — Ctx.Provider
- `src/transport/connection/client-context.web.tsx` — Ctx.Provider, PageClientCtx.Provider
- `src/worktree/host-workspace-list-states.tsx` — MobileNoMatchState, MobileStateView, Text, View
