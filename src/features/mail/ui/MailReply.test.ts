// @vitest-environment happy-dom
import { act, createElement, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../../integrations/harness/core/types";
import type { MailMessage } from "../model/mail";

const harness = vi.hoisted(() => ({
  run: vi.fn(),
  route: vi.fn(() => ["claude", "devin"]),
}));
const api = vi.hoisted(() => ({
  send: vi.fn(async () => undefined),
  draft: vi.fn(async () => undefined),
  recipients: vi.fn(),
}));
vi.mock("../../../integrations/harness/core/registry", () => ({
  canRunHarnessTextPrompt: () => true,
  runHarnessTextPrompt: harness.run,
}));
vi.mock("../../../integrations/harness/core/textHarness", () => ({
  pickTextHarness: () => "claude",
  routeTextHarnesses: () => harness.route(),
}));
vi.mock("../../../platform/tauri/fs", () => ({ homeDir: async () => "/home/me" }));
vi.mock("../model/mailApi", () => ({ mailApi: api }));
// Faces and icons come from the web; here every face is its initial.
vi.mock("../model/mailAvatar", async (original) => ({
  ...(await original<typeof import("../model/mailAvatar")>()),
  gravatarUrl: async () => undefined,
}));
vi.mock("../../sessions/ui/CitationMonogram", () => ({
  CitationMonogram: (props: { host: string }) =>
    createElement("span", { "data-testid": "org" }, props.host),
}));
// The word reveal has its own test; here it shows the text and settles as
// soon as the stream ends, as the real one does once its last word lands.
vi.mock("../../sessions/ui/StreamedPlainText", () => ({
  StreamedPlainText: (props: { text: string; streaming: boolean; onSettled?: () => void }) => {
    useEffect(() => {
      if (!props.streaming) props.onSettled?.();
    }, [props.streaming]);
    return createElement("div", { "data-testid": "streamed" }, props.text);
  },
}));

const { MailReply } = await import("./MailReply");

const message: MailMessage = {
  id: "a.inbox.7",
  accountId: "a",
  folder: "inbox",
  from: "NoHaySinSuerte - Jorge Luna",
  address: "info@jorgitoluna.com",
  subject: "Feliz cumpleaños",
  preview: "Tienes un regalo",
  body: ["Tienes un regalo."],
  at: Date.now(),
  unread: false,
  starred: false,
  triage: "routine",
  labels: [],
};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  harness.run.mockReset();
  harness.route.mockReturnValue(["claude", "devin"]);
  api.send.mockClear();
  api.draft.mockClear();
  api.recipients.mockResolvedValue({
    to: [{ name: "Soporte", address: "ayuda@jorgitoluna.com" }],
    cc: [{ name: "Luis", address: "luis@ejemplo.test" }],
  });
  vi.stubGlobal("requestAnimationFrame", (paint: FrameRequestCallback) =>
    setTimeout(() => paint(0), 0),
  );
  vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

const flush = () => act(async () => await new Promise((resolve) => setTimeout(resolve, 0)));
const box = () => container.querySelector("textarea");
/** The streamed text: with `subject`, the large line over it. */
const streamed = (subject = false) => {
  const all = [...container.querySelectorAll('[data-testid="streamed"]')];
  return subject ? (all.length > 1 ? all[0] : null) : (all.at(-1) ?? null);
};
const subjectField = () => container.querySelector<HTMLInputElement>('input[aria-label="Subject"]');
const button = (label: string) =>
  [...container.querySelectorAll("button")].find((entry) => entry.textContent?.trim() === label)!;
const account = { id: "a", name: "Ana", address: "ana@ejemplo.test", provider: "imap" };
const render = () =>
  act(() =>
    root.render(createElement(MailReply, { message, account, onClose: () => {}, onSent: () => {} })),
  );

describe("MailReply", () => {
  it("names who it goes to, and adds the copies only on Reply all", async () => {
    render();
    await flush();
    expect(api.recipients).toHaveBeenCalledWith("a.inbox.7", true);
    expect(container.textContent).toContain("Soporte");
    expect(container.textContent).toContain("ayuda@jorgitoluna.com");
    expect(container.textContent).not.toContain("Luis");
    // From the user's account, with the sender's organisation beside them.
    expect(container.textContent).toContain("ana@ejemplo.test");
    expect([...container.querySelectorAll('[data-testid="org"]')].map((el) => el.textContent)).toEqual([
      "ejemplo.test",
      "jorgitoluna.com",
    ]);
    expect(subjectField()?.value).toBe("Re: Feliz cumpleaños");
    act(() => button("Reply all").click());
    expect(container.textContent).toContain("Luis");

    vi.useFakeTimers();
    const field = box()!;
    act(() => {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
      setter.call(field, "Gracias");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    act(() => button("Send").click());
    await act(async () => vi.advanceTimersByTime(5_000));
    expect(api.send).toHaveBeenCalledWith("a.inbox.7", "Gracias", true, "Re: Feliz cumpleaños");
  });

  it("streams the AI subject large over its reply, then hands both over to edit", async () => {
    let emit!: (event: HarnessEvent) => void;
    let finish!: (output: string) => void;
    harness.run.mockImplementation(
      (input: { onEvent: (event: HarnessEvent) => void }) =>
        new Promise<string>((resolve) => {
          emit = input.onEvent;
          finish = resolve;
        }),
    );
    render();
    await flush();
    act(() => button("Write").click());
    await flush();

    expect(box()).toBeNull();
    expect(container.textContent).toContain("Reading the email…");
    expect(container.querySelector(".ai-glow")).not.toBeNull();
    expect(button("Stop")).toBeDefined();

    act(() => emit({ type: "message.delta", text: "Subject: Gracias por el regalo" }));
    await flush();
    expect(streamed(true)?.textContent).toBe("Gracias por el regalo");
    expect(streamed()?.textContent).toBe("");

    act(() => emit({ type: "message.delta", text: "\n\nHola Jorge," }));
    await flush();
    expect(streamed()?.textContent).toBe("Hola Jorge,");

    act(() => emit({ type: "message.delta", text: "\n\nGracias por [fact]." }));
    await flush();
    // The closing snapshot of the whole message must not double the text.
    const whole = "Subject: Gracias por el regalo\n\nHola Jorge,\n\nGracias por [fact].";
    act(() => emit({ type: "message.delta", text: whole }));
    await flush();
    expect(streamed()?.textContent).toBe("Hola Jorge,\n\nGracias por [fact].");
    expect(container.querySelector(".ai-glow")).not.toBeNull();

    // The message is whole: the border goes out with it, not when the agent exits.
    act(() => emit({ type: "message.completed" } as HarnessEvent));
    await flush();
    expect(container.querySelector(".ai-glow")).toBeNull();
    expect(button("Stop")).toBeDefined();

    await act(async () => finish(`${whole}\n\nSaludos`));
    await flush();
    expect(container.querySelector('[data-testid="streamed"]')).toBeNull();
    expect(subjectField()?.value).toBe("Gracias por el regalo");
    expect(box()?.value).toBe("Hola Jorge,\n\nGracias por [fact].\n\nSaludos");
    expect(container.querySelector(".ai-glow")).toBeNull();
    expect(container.textContent).toContain("Written with AI");
    expect(container.textContent).toContain("1 to fill");

    act(() => button("Undo").click());
    expect(box()?.value).toBe("");
    expect(subjectField()?.value).toBe("Re: Feliz cumpleaños");
  });

  it("stops on request and keeps what was written", async () => {
    let emit!: (event: HarnessEvent) => void;
    harness.run.mockImplementation(
      (input: { onEvent: (event: HarnessEvent) => void; signal: AbortSignal }) =>
        new Promise<string>((_, reject) => {
          emit = input.onEvent;
          input.signal.addEventListener("abort", () => reject(new Error("cancelled")));
        }),
    );
    render();
    await flush();
    act(() => button("Write").click());
    await flush();
    act(() => emit({ type: "message.delta", text: "Hola" }));
    await flush();
    act(() => button("Stop").click());
    await flush();
    expect(box()?.value).toBe("Hola");
    expect(container.querySelector(".ai-glow")).toBeNull();
    expect(container.textContent).toContain("Written with AI");
  });

  it("rotates automatically to the next provider when Claude hits its usage limit", async () => {
    const tried: string[] = [];
    harness.run.mockImplementation(
      async (input: { harness: string; onEvent: (event: HarnessEvent) => void }) => {
        tried.push(input.harness);
        if (input.harness === "claude") {
          throw new Error(
            "Claude Code reached its usage limit. Choose another available model or retry when the limit resets.",
          );
        }
        const reply = "Subject: Re: Feliz cumpleaños\n\nHola desde Devin!";
        input.onEvent({ type: "message.delta", text: reply });
        input.onEvent({ type: "message.completed" });
        return reply;
      },
    );
    render();
    await flush();
    act(() => button("Write").click());
    await flush();
    await flush();
    expect(tried).toEqual(["claude", "devin"]);
    expect(box()?.value).toBe("Hola desde Devin!");
  });
});
