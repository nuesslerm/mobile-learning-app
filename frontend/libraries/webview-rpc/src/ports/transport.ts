/** The one capability the protocol needs: send a string to the other side and hear the strings it sends back. */
export type RpcTransport = {
  send(raw: string): void;
  /** Starts listening; the returned function stops it. */
  listen(onRaw: (raw: string) => void): () => void;
};
