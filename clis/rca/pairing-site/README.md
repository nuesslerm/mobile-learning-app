# RCA phone pairing site

This is the private Mac-local setup site. It serves HTML and simulator screenshots and calls the existing pairing commands for `t3-serve` and `orca-serve`. Those services own device state and handle the apps' traffic after pairing.

The Mac's existing pairing LaunchAgent runs `~/Library/Application Support/RCA Local/pairing-page.py`. That entry point loads the installed copy at `~/Library/Application Support/RCA Local/pairing-site/server.py`. The existing LaunchAgent provides `PAIR_HOST` and `PAIR_OWNER`. The server binds to loopback on port 18798, expects Tailscale Serve to supply the owner's identity, and stops serving after two hours. It uses the binaries and state directories one level above its installed folder. Set `PAIR_STATE` to point elsewhere when testing.

To add an app, register its route, name, description, and credential handler in `APPS` in `server.py`. Add its guide under `pages/` with the same name as the route, and give its credential handler a result page. The landing page and routing use the registry automatically.
