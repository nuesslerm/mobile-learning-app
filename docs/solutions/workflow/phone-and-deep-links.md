---
title: "Links shared with the user are markdown links, and the phone link is an EAS Update"
date: 2026-11-08
category: workflow
module: agent-instructions
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "An agent shares a URL or a deep link with the user"
  - "An agent needs a link for the phone"
tags: [links, eas-update, expo]
---

# Links shared with the user

Moved verbatim from the always-on `CLAUDE.md` section "Links" (8 Nov 2026), where a short rule now points here.

Every URL or deep link shared with the user is a markdown link with a short label, never bare. The phone link is an EAS Update, never a live Metro: the `### EAS Update` link of the pushed commit's CI run summary (a push that left the app unchanged publishes none: `wf ship eas --latest` prints the link of CI's last publish of `dev`), or the `link` that `wf ship eas --initial-route <route>` prints for a branch (an immutable update: no Fast Refresh, and by hand it bakes only the `EXPO_PUBLIC_*` variables of your environment). GitHub strips `exp://` out of markdown links, so in a PR or issue body link the https `expo.dev` update link and keep the raw `exp://u.expo.dev/<projectId>/group/<group>` deep link in a code span beside it.
