import { describe, expect, it, vi } from "vitest";
import { trimReplay } from "./pty";

const KB = 1024;

describe("trimReplay", () => {
  it("keeps a small buffer whole", () => {
    const sizes = [KB, KB, KB];
    expect(trimReplay(sizes, 3 * KB)).toEqual({ drop: 0, bytes: 3 * KB });
  });

  it("drops oldest chunks once the byte budget is exceeded", () => {
    // Ten 32KB chunks is 320KB, over the 256KB budget.
    const sizes = Array(10).fill(32 * KB);
    const { drop, bytes } = trimReplay(sizes, 320 * KB);
    expect(drop).toBe(2);
    expect(bytes).toBe(256 * KB);
  });

  it("bounds a flood of tiny chunks by count", () => {
    const sizes = Array(250).fill(4);
    const { drop } = trimReplay(sizes, 1000);
    expect(sizes.length - drop).toBe(200);
  });

  it("keeps the newest chunk even when it alone exceeds the budget", () => {
    const sizes = [KB, 512 * KB];
    const { drop, bytes } = trimReplay(sizes, 513 * KB);
    expect(drop).toBe(1);
    expect(bytes).toBe(512 * KB);
  });

  it("never drops the only chunk", () => {
    const sizes = [512 * KB];
    expect(trimReplay(sizes, 512 * KB)).toEqual({ drop: 0, bytes: 512 * KB });
  });
});

describe("spawnPty output channel", () => {
  it("delivers raw bytes from the latest spawn and drops a replaced one", async () => {
    vi.resetModules();
    const sent: Array<(data: ArrayBuffer) => void> = [];
    vi.doMock("@tauri-apps/api/core", () => ({
      Channel: class {
        constructor(onmessage: (data: ArrayBuffer) => void) {
          sent.push(onmessage);
        }
      },
      invoke: vi.fn(async () => undefined),
    }));
    vi.doMock("@tauri-apps/api/event", () => ({
      listen: vi.fn(async () => () => {}),
    }));
    const pty = await import("./pty");
    const received: number[][] = [];
    pty.subscribePty("t1", (chunk) => received.push(Array.from(chunk)), () => {});
    await pty.spawnPty("t1", "/tmp", 80, 24);
    await pty.spawnPty("t1", "/tmp", 80, 24);
    sent[0](new Uint8Array([1]).buffer);
    sent[1](new Uint8Array([104, 105]).buffer);
    expect(received).toEqual([[104, 105]]);
    vi.doUnmock("@tauri-apps/api/core");
    vi.doUnmock("@tauri-apps/api/event");
  });
});
