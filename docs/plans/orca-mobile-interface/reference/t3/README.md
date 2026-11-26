# Native T3 traffic on the RCA simulator

The installed T3 mobile app paired through a loopback Mimic proxy to the genuine server from the pinned npm distribution. The server used its own state directory and a temporary Git repository. The client retained its existing environments. Capture includes discovery and pairing, home, reconnect, and opening an idle fixture thread. No provider turn ran during these checkpoints.

[Observed operations](native-ios-operations.json) lists the first occurrence of each HTTP route and RPC method in wire order within each capture. HTTP statuses come from the same exchange. A socket reply must match both its connection and request label. The two captures have independent sequence numbers. A streaming request can remain open after its first chunk.

To reproduce, start the pinned server on loopback, point the proxy at its origin, and add an environment in the native app using the proxy address and a fresh token from the reference server. Open a thread belonging to that environment. Supplying the server address directly in the mobile form bypasses the capture. The contract pin and integrity digest are in [the conformance manifest](../../../../../tooling/t3-conformance/t3-pin.json).

## Expo against RCA

The Coding agent drawer entry opens `/coding-agent`. Both the home route and existing thread URLs use the Orca interface and restore the saved T3 connection. A device paired only to the older RCA gateway needs a T3 pairing invite. On the exported mobile web app, the RCA-backed server accepted a pairing grant, returned the fixture project and opened its thread using a fresh RPC ticket. Sending through the composer dispatched a real provider turn. The transcript displayed the streamed reply and restored it after reloading. The [restored chat](mla-web-chat.png) was captured with the keyboard dismissed. Native visual matching, uploads and interrupted-turn recovery remain open.

The native Expo release build also completed pairing on the RCA iPhone 18 Pro Max simulator. It used its own fresh invite to the same server, subscribed to the same thread and displayed the provider reply previously sent from the browser. See the [native Expo chat](mla-ios-chat.png). The header, background and composer still differ from the [Orca chat reference](../ios/paired-chat/photo-thread.png).

The T3 bounded thread read accepts the RCA option `rcaChatLayout=1`. It adds `rcaToolTextOffsets`, keyed by tool item ID. Each offset is a UTF-16 position in its run’s assistant text when the first tool frame arrived. RCA rebuilds this index from persisted T3 events on startup. The ordinary bounded response retains its upstream shape. The client resumes the subscription at `snapshotSequence` and tracks the same positions as new tools arrive. See the [native photo comparison](../ios/paired-chat/README.md).
