//! Minimal graphql-ws client for subscriptions (protocol: "graphql-transport-ws").

export type ConnectionStatus =
  | { state: "connecting" }
  | { state: "open" }
  | { state: "closed"; reason: string };

export interface SubscribeOptions<TData> {
  query: string;
  variables?: Record<string, unknown>;
  onData: (data: TData) => void;
  onError?: (err: unknown) => void;
  onStatus?: (status: ConnectionStatus) => void;
}

export interface SubscriptionHandle {
  close: () => void;
}

let idCounter = 0;

export function subscribe<TData>(opts: SubscribeOptions<TData>): SubscriptionHandle {
  const id = String(++idCounter);
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  const url = `${proto}//${window.location.host}/graphql/ws`;
  const ws = new WebSocket(url, "graphql-transport-ws");
  let closed = false;
  opts.onStatus?.({ state: "connecting" });

  ws.addEventListener("open", () => {
    ws.send(JSON.stringify({ type: "connection_init" }));
  });

  ws.addEventListener("message", (ev) => {
    try {
      const msg = JSON.parse(ev.data as string);
      switch (msg.type) {
        case "connection_ack":
          opts.onStatus?.({ state: "open" });
          ws.send(
            JSON.stringify({
              id,
              type: "subscribe",
              payload: { query: opts.query, variables: opts.variables ?? {} },
            }),
          );
          break;
        case "next":
          if (msg.id === id && msg.payload?.data) {
            opts.onData(msg.payload.data as TData);
          }
          break;
        case "error":
          opts.onError?.(msg.payload);
          break;
        case "complete":
          // server finished
          break;
      }
    } catch (e) {
      opts.onError?.(e);
    }
  });

  ws.addEventListener("error", (e) => opts.onError?.(e));
  ws.addEventListener("close", (ev) => {
    opts.onStatus?.({
      state: "closed",
      reason: ev.reason || `code ${ev.code}`,
    });
  });

  return {
    close: () => {
      if (closed) return;
      closed = true;
      try {
        ws.send(JSON.stringify({ id, type: "complete" }));
      } catch {
        /* ignore */
      }
      ws.close();
    },
  };
}
