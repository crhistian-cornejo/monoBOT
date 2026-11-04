// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ControlOutcome } from "../../orchestration/model/orchestration";
import type { Session } from "../../sessions/model/session";

// The mic: the test speaks by calling what the call gave dictation.
const mic = {
  heard: (_text: string) => {},
  toggle: vi.fn(),
  cancel: vi.fn(),
};
vi.mock("../../dictation/ui/useDictation", () => ({
  useDictation: (onText: (text: string) => void) => {
    mic.heard = onText;
    return {
      phase: "idle",
      levels: [],
      partial: "",
      speaking: false,
      progress: 0,
      modelBytes: 0,
      toggle: mic.toggle,
      install: vi.fn(),
      cancel: mic.cancel,
      dismiss: vi.fn(),
    };
  },
}));
vi.mock("../model/voice", () => ({
  voiceStatus: async () => ({ installed: true, installing: false, bytes: 1 }),
  voiceSpeakers: async () => 10,
  installVoice: vi.fn(),
  onVoiceInstall: vi.fn(async () => () => {}),
  speakSentence: vi.fn(),
}));
// The speaker: records what it was asked to say, and plays it at once.
const played: { text: string; lang: string; voice: number }[] = [];
const silenced = vi.fn();
vi.mock("./callPlayer", () => ({
  CallPlayer: class {
    say(text: string, lang: string, voice: number, onStart?: () => void) {
      played.push({ text, lang, voice });
      onStart?.();
    }
    drained() {
      return Promise.resolve();
    }
    stop() {
      silenced();
    }
    close() {}
  },
}));

const { useGroupCall } = await import("./useGroupCall");
type Options = Parameters<typeof useGroupCall>[0];
type State = ReturnType<typeof useGroupCall>;

const members = [
  { id: "ana", name: "Ana" },
  { id: "leo", name: "Leo" },
  { id: "sofi", name: "Sofi" },
];

type Asked = {
  memberId: string;
  prompt: string;
  requestId: string;
  settle: (outcome: ControlOutcome) => void;
};

let root: Root;
let call: State;
let asked: Asked[];
let replies: Record<string, string | undefined>;
let options: Options;

function Harness(props: Options) {
  call = useGroupCall(props);
  return null;
}

async function render(patch: Partial<Options> = {}) {
  options = { ...options, ...patch };
  await act(async () => root.render(createElement(Harness, options)));
}

async function settle() {
  await act(async () => {
    for (let index = 0; index < 20; index++) await Promise.resolve();
  });
}

/** Waits for the call to get somewhere, letting its timers run under act a
 * slice at a time, so React renders between checks. */
async function until(check: () => void) {
  for (let slice = 0; ; slice++) {
    await act(() => new Promise((resolve) => setTimeout(resolve, 25)));
    try {
      return check();
    } catch (error) {
      if (slice > 120) throw error;
    }
  }
}

async function startCall() {
  await render();
  await act(async () => call.start());
  await settle();
  expect(call.phase).toBe("listening");
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.clearAllMocks();
  played.length = 0;
  asked = [];
  replies = {};
  options = {
    group: { id: "trip", name: "Trip", goal: "Plan the trip" },
    members,
    sessions: [],
    // A member answers with its canned reply, unless the test settles it.
    ask: vi.fn(async (memberId: string, prompt: string, requestId: string) => {
      const outcome = new Promise<ControlOutcome>((resolve) => {
        asked.push({ memberId, prompt, requestId, settle: resolve });
        const reply = replies[memberId];
        if (reply !== undefined) resolve({ status: "completed", text: reply });
      });
      return { sessionId: `session-${memberId}`, outcome };
    }),
    stop: vi.fn(),
    save: vi.fn(),
  };
  root = createRoot(document.createElement("div"));
});

afterEach(async () => {
  await act(async () => root.unmount());
  vi.unstubAllGlobals();
});

describe("a group call", () => {
  it("opens the mic, then asks each member in turn and speaks their answers", async () => {
    replies = {
      ana: "Sí, el viernes hay vuelos baratos. Yo reservaría ya.",
      leo: "PASS",
      sofi: "Llevemos abrigo, hace frío en Cusco.",
    };
    await startCall();
    expect(mic.toggle).toHaveBeenCalled();

    await act(async () => mic.heard("¿Qué hacemos el viernes?"));
    await until(() => expect(call.phase).toBe("listening"));

    expect(asked.map((entry) => entry.memberId)).toEqual([
      "ana",
      "leo",
      "sofi",
    ]);
    expect(asked[0].prompt).toContain(
      "The user said: ¿Qué hacemos el viernes?",
    );
    expect(asked[1].prompt).toContain(
      "Ana said: Sí, el viernes hay vuelos baratos. Yo reservaría ya.",
    );
    // Leo passed, so Sofi hears Ana only.
    expect(asked[2].prompt).toContain("Ana said:");
    expect(asked[2].prompt).not.toContain("Leo said:");

    expect(played.map((entry) => entry.text)).toEqual([
      "Sí, el viernes hay vuelos baratos.",
      "Yo reservaría ya.",
      "Llevemos abrigo, hace frío en Cusco.",
    ]);
    expect(played.every((entry) => entry.lang === "es")).toBe(true);
    // Each member speaks in a voice of its own.
    expect(played[0].voice).toBe(played[1].voice);
    expect(played[2].voice).not.toBe(played[0].voice);

    expect(
      call.lines.map((line) => [line.name, line.status, line.text]),
    ).toEqual([
      ["You", "said", "¿Qué hacemos el viernes?"],
      ["Ana", "said", "Sí, el viernes hay vuelos baratos. Yo reservaría ya."],
      ["Leo", "passed", ""],
      ["Sofi", "said", "Llevemos abrigo, hace frío en Cusco."],
    ]);
  });

  it("lets a named member answer alone and starts the next round after it", async () => {
    replies = {
      leo: "I'd go on Saturday.",
      ana: "Friday works.",
      sofi: "PASS",
    };
    await startCall();
    await act(async () => mic.heard("Leo, what do you think?"));
    await until(() => expect(call.phase).toBe("listening"));
    expect(asked.map((entry) => entry.memberId)).toEqual(["leo"]);
    expect(asked[0].prompt).not.toContain("PASS");
    expect(played).toEqual([
      { text: "I'd go on Saturday.", lang: "en", voice: expect.any(Number) },
    ]);

    // Without a name, everyone answers, and Leo, who went first by name,
    // does not move the rotation.
    await act(async () => mic.heard("And the hotel?"));
    await until(() => expect(asked).toHaveLength(4));
    expect(asked.slice(1).map((entry) => entry.memberId)).toEqual([
      "ana",
      "leo",
      "sofi",
    ]);
  });

  it("speaks a reply while it is still streaming", async () => {
    await startCall();
    await act(async () => mic.heard("Hola"));
    await until(() => expect(asked).toHaveLength(1));
    const { requestId, settle: finish } = asked[0];
    const session = (text: string, streaming = true): Session =>
      ({
        id: "session-ana",
        blocks: [
          { id: "u", role: "user", text: "call", appRequestId: requestId },
          { id: "a", role: "assistant", text, streaming },
        ],
      }) as unknown as Session;

    // "PA" might be PASS: nothing is said yet.
    await render({ sessions: [session("PA")] });
    expect(played).toEqual([]);
    await render({ sessions: [session("Hola, soy Ana. Creo que")] });
    expect(played.map((entry) => entry.text)).toEqual(["Hola, soy Ana."]);

    const full = "Hola, soy Ana. Creo que sí.";
    await render({ sessions: [session(full, false)] });
    await act(async () => finish({ status: "completed", text: full }));
    await until(() => expect(asked).toHaveLength(2));
    expect(played.map((entry) => entry.text)).toEqual([
      "Hola, soy Ana.",
      "Creo que sí.",
    ]);
  });

  it("stops the members when the user interrupts, and skips a busy one", async () => {
    options.ask = vi.fn(
      async (memberId: string, prompt: string, requestId: string) => {
        if (memberId === "ana")
          throw new Error("It is in the middle of something else");
        const outcome = new Promise<ControlOutcome>((resolve) =>
          asked.push({ memberId, prompt, requestId, settle: resolve }),
        );
        return { sessionId: `session-${memberId}`, outcome };
      },
    );
    await startCall();
    await act(async () => mic.heard("¿Alguien?"));
    await until(() =>
      expect(asked.map((entry) => entry.memberId)).toEqual(["leo"]),
    );
    expect(call.lines[1]).toMatchObject({ name: "Ana", status: "busy" });
    expect(call.thinkingId).toBe("leo");

    await act(async () => call.interrupt());
    expect(call.phase).toBe("listening");
    expect(silenced).toHaveBeenCalled();
    expect(options.stop).toHaveBeenCalledWith("session-leo");
    expect(call.lines[2]).toMatchObject({ name: "Leo", status: "stopped" });

    // Leo's late answer belongs to the dropped round: no one says it.
    await act(async () =>
      asked[0].settle({ status: "cancelled", text: "Late." }),
    );
    await settle();
    expect(played).toEqual([]);
    expect(asked).toHaveLength(1);
  });

  it("saves the transcript when the user hangs up", async () => {
    replies = { ana: "Vamos el viernes.", leo: "PASS", sofi: "PASS" };
    await startCall();
    await act(async () => mic.heard("¿Cuándo vamos?"));
    await until(() => expect(call.phase).toBe("listening"));
    await act(async () => call.hangUp());
    expect(call.phase).toBe("idle");
    expect(options.save).toHaveBeenCalledTimes(1);
    const [title, body] = vi.mocked(options.save!).mock.calls[0];
    expect(title).toMatch(/^Call with Trip, /);
    expect(body).toContain("**You:** ¿Cuándo vamos?");
    expect(body).toContain("**Ana:** Vamos el viernes.");
    // A call that is over hears nothing more.
    await act(async () => mic.heard("¿Hola?"));
    expect(asked).toHaveLength(3);
  });
});
