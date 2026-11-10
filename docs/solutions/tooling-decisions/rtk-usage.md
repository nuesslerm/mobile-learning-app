---
title: "RTK: call rtk tools directly, never pipe them through head, tail or cat"
date: 2026-11-08
category: tooling-decisions
module: rtk
problem_type: workflow_issue
component: development_workflow
severity: low
applies_when:
  - "An agent runs git, ls or grep through the RTK hook"
tags: [rtk, hooks]
---

# RTK usage

Moved verbatim from the always-on `CLAUDE.md` bullet (8 Nov 2026).

- RTK: call `rtk <tool>` directly (`rtk git status`, `rtk ls dir`, `rtk grep pat src`) -- it truncates and paginates already, so never append `| head`/`| tail`/`| cat`; the hook blocks that shape and names the command to run. A pipe or redirect only skips the token compression, never the execution, so keep a pipeline when the downstream tool consumes the bytes (`| wc -l`, `| jq`) and filter with the tool's own flags otherwise.
