# Mobile Browser MCP Eval Checklist

- starts in mobile-framed mode
- uses `app_*` for normal interaction
- uses `shell_*` only for frame diagnostics
- verifies keyboard ownership in the app iframe
- uses `session_reset` / `session_restart` for local recovery
- closes sessions cleanly
