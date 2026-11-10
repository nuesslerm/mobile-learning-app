---
status: blocked
area: vnc-auth
paths:
  - "clis/wf-login/src/vnc/descriptor.rs"
opened: 2026-10-04
last_verified: 2026-10-04
links: []
---
# Reddit blocks the auth desktop login page from the VM datacenter IP

## What is broken

Reddit's login renders correctly but shows "Your request has been blocked by network security"; GitHub,
Amazon, Stack Overflow and X load fine from the same browser in the same minute.

## Why blocked

The VM's egress is an Oracle datacenter range, which Reddit blocks on the login path.

## What unblocks it

Residential or mobile egress, not worth acquiring for one provider. A Reddit descriptor should be
expected to fail at the network layer, not debugged as rendering.
