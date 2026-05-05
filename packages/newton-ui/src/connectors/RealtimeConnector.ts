export interface RealtimeMessage {
  type: string;
  payload: unknown;
}

export class RealtimeConnector {
  private ws: WebSocket | null = null;

  constructor(private url: string) {}

  connect(onMessage: (m: RealtimeMessage) => void): void {
    this.ws = new WebSocket(this.url);
    this.ws.onmessage = (ev) => {
      try {
        onMessage(JSON.parse(ev.data) as RealtimeMessage);
      } catch {
        // ignore malformed frames
      }
    };
  }

  disconnect(): void {
    this.ws?.close();
    this.ws = null;
  }
}
