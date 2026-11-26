# Observe the native T3 client against the pinned server

This runbook records the HTTP exchanges and WebSocket envelopes used by the T3 mobile app. It uses the iOS simulator named `rca`, a dedicated upstream state directory and a loopback proxy. Keep the proxy running while the app pairs and opens a thread.

1. Start the real T3 distribution specified in [the conformance pin](../../tooling/t3-conformance/t3-pin.json) against a fixture repository. Record its version from `/.well-known/t3/environment`. The RCA implementation can subsequently run the same journey on a different port.
2. Start the recorder. Choose a new output filename for each run.

   ```sh
   wf mimic proxy --upstream http://127.0.0.1:18498 --listen 127.0.0.1:18499 \
     --out /tmp/t3-pair-and-chat.jsonl --duration 300
   ```

3. Mint a fresh token using the reference server’s `t3 pair --base-dir <reference-state>`. In the installed T3 app, open Settings → Environments → Add environment. Enter the **proxy address** and the one-time code, then submit. The simulator can reach the Mac loopback. For automated native form entry, target the installed app with `WF_IOS_APP_ID=com.t3tools.t3code`.
4. Open a thread in the fixture environment. Review the HTTP status for descriptor discovery, OAuth exchange, RPC ticket issuance and shell bootstrap. Identify the RPC subscriptions and commands that opening the thread caused.
5. Let the duration elapse or send SIGTERM to the recorder process you started. Read the footer and exit status. To correlate a socket reply with a request, match the connection and request labels.

   ```sh
   jq -c 'select(.kind == "ws.frame" and .envelope == "Request") | {seq,connection,request,method,payload}' /tmp/t3-pair-and-chat.jsonl
   jq -c 'select(.kind == "http.response") | {seq,exchange,status,body}' /tmp/t3-pair-and-chat.jsonl
   ```

The export preserves protocol field names, types and envelope order. Unknown names and leaf values are masked. Credentials still reach the configured local upstream so the app can authenticate. Inspect the [observed native operations](../plans/orca-mobile-interface/reference/t3/native-ios-operations.json) for the recorded pairing and thread-opening sequence. Parallel requests, cached state and background refreshes can change the order between runs.
