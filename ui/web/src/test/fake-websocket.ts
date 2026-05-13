import type { WebSocketLike } from "../lib/websocket";
import type {
  DecodedFrame,
  DecoderRegistry,
  DecoderRegistryFrameCallback,
} from "../lib/preview-decoder";

export class FakeWebSocket extends EventTarget implements WebSocketLike {
  binaryType: BinaryType = "blob";
  readyState = 0;
  readonly sent: string[] = [];
  readonly url: string;

  constructor(url: string) {
    super();
    this.url = url;
  }

  open(): void {
    this.readyState = 1;
    this.dispatchEvent(new Event("open"));
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    this.readyState = 3;
    this.dispatchEvent(new Event("close"));
  }

  emitMessage(data: unknown): void {
    this.dispatchEvent(new MessageEvent("message", { data }));
  }
}

export function createFakeWebSocketFactory() {
  const sockets: FakeWebSocket[] = [];
  return {
    sockets,
    factory(url: string) {
      const socket = new FakeWebSocket(url);
      sockets.push(socket);
      return socket;
    },
  };
}

/**
 * Test double for `DecoderRegistry`. Records every `configure` /
 * `decode` / `close` / `closeAll` call and exposes a `synthesizeFrame`
 * helper so tests can simulate `VideoDecoder` output without needing
 * the WebCodecs API in jsdom.
 */
export interface RecordedConfigureCall {
  name: string;
  codecId: number;
  description: Uint8Array;
  width: number;
  height: number;
}

export interface RecordedDecodeCall {
  name: string;
  payload: Uint8Array;
  ptsUs: number;
  sourceTimestampUs: number;
  isKeyframe: boolean;
}

export class FakeDecoderRegistry implements DecoderRegistry {
  readonly configureCalls: RecordedConfigureCall[] = [];
  readonly decodeCalls: RecordedDecodeCall[] = [];
  readonly closedNames: string[] = [];
  closeAllCount = 0;
  private readonly callbacks = new Map<
    string,
    DecoderRegistryFrameCallback
  >();
  private readonly dims = new Map<
    string,
    { width: number; height: number }
  >();

  configure(
    name: string,
    codecId: number,
    description: Uint8Array,
    width: number,
    height: number,
    onFrame: DecoderRegistryFrameCallback,
  ): void {
    this.configureCalls.push({ name, codecId, description, width, height });
    this.callbacks.set(name, onFrame);
    this.dims.set(name, { width, height });
  }

  decode(
    name: string,
    payload: Uint8Array,
    ptsUs: number,
    sourceTimestampUs: number,
    isKeyframe: boolean,
  ): void {
    this.decodeCalls.push({ name, payload, ptsUs, sourceTimestampUs, isKeyframe });
  }

  close(name: string): void {
    this.closedNames.push(name);
    this.callbacks.delete(name);
    this.dims.delete(name);
  }

  closeAll(): void {
    this.closeAllCount += 1;
    this.callbacks.clear();
    this.dims.clear();
  }

  /** Simulate `VideoDecoder` producing one decoded frame for `name`.
   *  Requires that `configure(name, ...)` has been called. */
  synthesizeFrame(
    name: string,
    overrides: Partial<Omit<DecodedFrame, "name">> = {},
  ): DecodedFrame | null {
    const callback = this.callbacks.get(name);
    if (!callback) {
      return null;
    }
    const dims = this.dims.get(name) ?? { width: 0, height: 0 };
    const fakeVideoFrame = {
      timestamp: overrides.timestampUs ?? 0,
      close() {
        /* test double */
      },
    } as unknown as VideoFrame;
    const frame: DecodedFrame = {
      name,
      videoFrame: overrides.videoFrame ?? fakeVideoFrame,
      width: overrides.width ?? dims.width,
      height: overrides.height ?? dims.height,
      timestampUs: overrides.timestampUs ?? 0,
      sourceTimestampUs: overrides.sourceTimestampUs ?? Date.now() * 1000,
      receivedAtWallTimeMs: overrides.receivedAtWallTimeMs ?? Date.now(),
    };
    callback(frame);
    return frame;
  }
}
