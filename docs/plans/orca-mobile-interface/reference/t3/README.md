# Native T3 traffic on the RCA simulator

The installed T3 mobile app paired through a loopback Mimic proxy to the genuine server from the pinned npm distribution. The server used its own state directory and a temporary Git repository. The client retained its existing environments. Capture includes discovery and pairing, home, reconnect, and opening an idle fixture thread. No provider turn ran during these checkpoints.

[Observed operations](native-ios-operations.json) lists the first occurrence of each HTTP route and RPC method in wire order within each capture. HTTP statuses come from the same exchange. A socket reply must match both its connection and request label. The two captures have independent sequence numbers. A streaming request can remain open after its first chunk.

To reproduce, start the pinned server on loopback, point the proxy at its origin, and add an environment in the native app using the proxy address and a fresh token from the reference server. Open a thread belonging to that environment. Supplying the server address directly in the mobile form bypasses the capture. The contract pin and integrity digest are in [the conformance manifest](../../../../../tooling/t3-conformance/t3-pin.json).

## Expo against RCA

The staged route is `/coding-agent?presentation=orca&transport=t3`. It has a separate saved connection for T3. On the exported mobile web app, the RCA-backed server accepted a pairing grant, returned the fixture project and opened its thread using a fresh RPC ticket. Sending through the composer dispatched a real provider turn. The transcript displayed the streamed reply and restored it after reloading. The [restored chat](mla-web-chat.png) was captured with the keyboard dismissed. Native visual matching, uploads and interrupted-turn recovery remain open.
