import { useCallback, useEffect, useRef, useState } from "react";
import type { ControlOutcome } from "../../orchestration/model/orchestration";
import type { Session } from "../../sessions/model/session";
import { useDictation, type Dictation } from "../../dictation/ui/useDictation";
import {
  CALL_MAX_SPEAKERS,
  CALL_TURN_GAP_MS,
  callMessage,
  callTranscript,
  callTurnOrder,
  couldBePass,
  defaultCallLanguage,
  isPass,
  memberReply,
  memberVoices,
  replyLanguage,
  takeSentences,
  type CallLanguage,
  type CallLine,
  type CallMember,
} from "../model/groupCall";
import {
  installVoice,
  onVoiceInstall,
  voiceSpeakers,
  voiceStatus,
} from "../model/voice";
import { CallPlayer } from "./callPlayer";

export type CallPhase =
  | "idle"
  | "starting"
  | "needs-voice"
  | "installing-voice"
  /** The mic is open for the user's turn. */
  | "listening"
  /** Members are answering; the mic is closed so they don't hear themselves. */
  | "answering";

/** A member took the call's message; its turn settles with `outcome`. */
export type MemberAsk = { sessionId: string; outcome: Promise<ControlOutcome> };

export type GroupCallOptions = {
  group: { id: string; name: string; goal?: string };
  members: readonly CallMember[];
  sessions: readonly Session[];
  /** Sends the call's message to the member's own conversation. */
  ask: (
    memberId: string,
    prompt: string,
    requestId: string,
  ) => Promise<MemberAsk>;
  /** Stops a member's answer the user talked over. */
  stop?: (sessionId: string) => void;
  /** Keeps the call's transcript among the group's documents. */
  save?: (title: string, body: string) => void | Promise<void>;
};

export type GroupCallState = {
  phase: CallPhase;
  lines: CallLine[];
  /** The member whose voice is playing. */
  speakingId?: string;
  /** The member working out its answer. */
  thinkingId?: string;
  muted: boolean;
  voiceBytes: number;
  voiceProgress: number;
  error?: string;
  /** The mic, as the composers use it. */
  dictation: Dictation;
  start: () => void;
  installVoices: () => void;
  hangUp: () => void;
  /** Silences the members and gives the turn back to the user. */
  interrupt: () => void;
  /** Ends the user's turn now rather than after the pause. */
  endTurn: () => void;
  toggleMute: () => void;
  /** A typed turn, in place of speaking. */
  say: (text: string) => void;
  dismiss: () => void;
};

type Stream = {
  requestId: string;
  sessionId: string;
  memberId: string;
  lineId: string;
  /** How far into the reply has been handed to the voice. */
  from: number;
  lang?: CallLanguage;
  fallback: CallLanguage;
  voice: number;
  spoken: string[];
};

let lineCount = 0;
const nextLineId = () => `call-line-${(lineCount++).toString(36)}`;
const reasonText = (reason: unknown) =>
  reason instanceof Error ? reason.message : String(reason);

/**
 * A voice call with a group. The user's turn is heard by the composers'
 * dictation and ends at a pause; each member is then asked in turn, its
 * reply spoken sentence by sentence while the next member works out its own.
 */
export function useGroupCall(options: GroupCallOptions): GroupCallState {
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const [phase, setPhase] = useState<CallPhase>("idle");
  const [lines, setLines] = useState<CallLine[]>([]);
  const linesRef = useRef<CallLine[]>([]);
  const [speakingId, setSpeakingId] = useState<string>();
  const [thinkingId, setThinkingId] = useState<string>();
  const [muted, setMuted] = useState(false);
  const [voiceBytes, setVoiceBytes] = useState(0);
  const [voiceProgress, setVoiceProgress] = useState(0);
  const [error, setError] = useState<string>();
  const live = useRef(false);
  const round = useRef(0);
  const player = useRef<CallPlayer | null>(null);
  const voices = useRef(new Map<string, number>());
  const streams = useRef(new Map<string, Stream>());
  const previousFirst = useRef<string | undefined>(undefined);
  const startedAt = useRef(0);
  const rendered = useRef<(() => void)[]>([]);
  const heard = useRef<(text: string) => void>(() => {});
  const dictation = useDictation((text) => heard.current(text));
  const dictationRef = useRef(dictation);
  dictationRef.current = dictation;

  const editLines = (change: (current: CallLine[]) => CallLine[]) => {
    linesRef.current = change(linesRef.current);
    setLines(linesRef.current);
  };
  const addLine = (line: Omit<CallLine, "id">) => {
    const id = nextLineId();
    editLines((current) => [...current, { ...line, id }]);
    return id;
  };
  const patchLine = (id: string, patch: Partial<CallLine>) =>
    editLines((current) =>
      current.map((line) => (line.id === id ? { ...line, ...patch } : line)),
    );
  const sessionOf = (id: string) =>
    optionsRef.current.sessions.find((session) => session.id === id);
  // The member's last words reach the page a render after its turn settles.
  const afterRender = () =>
    new Promise<void>((resolve) => {
      rendered.current.push(resolve);
      setTimeout(resolve, 250);
    });

  /** Hands the reply's finished sentences to the voice. */
  const pump = (stream: Stream, text: string, done: boolean) => {
    if (stream.from === 0 && !done && couldBePass(text)) return;
    if (done && stream.from === 0 && isPass(text)) return;
    const { sentences, next } = takeSentences(text, stream.from, done);
    stream.from = next;
    for (const sentence of sentences) {
      if (!stream.spoken.length && isPass(sentence)) continue;
      const lang = (stream.lang ??= replyLanguage(text, stream.fallback));
      stream.spoken.push(sentence);
      patchLine(stream.lineId, {
        text: stream.spoken.join(" "),
        status: "said",
      });
      player.current?.say(sentence, lang, stream.voice, () =>
        setSpeakingId(stream.memberId),
      );
    }
  };

  useEffect(() => {
    for (const stream of streams.current.values())
      pump(
        stream,
        memberReply(sessionOf(stream.sessionId), stream.requestId),
        false,
      );
    for (const resolve of rendered.current.splice(0)) resolve();
  }, [options.sessions]);

  const runRound = async (said: string) => {
    const id = ++round.current;
    const { group, members, ask } = optionsRef.current;
    setPhase("answering");
    const fallback = replyLanguage(
      said,
      defaultCallLanguage(globalThis.navigator?.language ?? "en"),
    );
    addLine({ who: "user", name: "You", text: said.trim(), status: "said" });
    const { order, addressed } = callTurnOrder(
      members,
      said,
      previousFirst.current,
    );
    if (!addressed && order[0]) previousFirst.current = order[0];
    const earlier: { name: string; text: string }[] = [];
    for (const memberId of order) {
      if (round.current !== id) return;
      if (!addressed && earlier.length >= CALL_MAX_SPEAKERS) break;
      const member = members.find((entry) => entry.id === memberId);
      if (!member) continue;
      const lineId = addLine({
        who: memberId,
        name: member.name,
        text: "",
        status: "thinking",
      });
      setThinkingId(memberId);
      const requestId = `call-${group.id}-${id}-${memberId}-${Date.now().toString(36)}`;
      let asked: MemberAsk;
      try {
        asked = await ask(
          memberId,
          callMessage({
            group: group.name,
            goal: group.goal,
            others: members
              .filter((entry) => entry.id !== memberId)
              .map((entry) => entry.name),
            said,
            earlier,
            addressed,
          }),
          requestId,
        );
      } catch (reason) {
        patchLine(lineId, { status: "busy", note: reasonText(reason) });
        continue;
      }
      if (round.current !== id) {
        optionsRef.current.stop?.(asked.sessionId);
        return;
      }
      const stream: Stream = {
        requestId,
        sessionId: asked.sessionId,
        memberId,
        lineId,
        from: 0,
        fallback,
        voice: voices.current.get(memberId) ?? 0,
        spoken: [],
      };
      streams.current.set(requestId, stream);
      pump(stream, memberReply(sessionOf(asked.sessionId), requestId), false);
      const outcome = await asked.outcome;
      await afterRender();
      streams.current.delete(requestId);
      if (round.current !== id) return;
      const reply =
        memberReply(sessionOf(asked.sessionId), requestId) ||
        (stream.from === 0 ? outcome.text : "");
      if (stream.from === 0 && isPass(reply)) {
        patchLine(lineId, { status: "passed" });
        continue;
      }
      pump(stream, reply, true);
      if (!stream.spoken.length) {
        patchLine(
          lineId,
          outcome.status === "completed"
            ? { status: "passed" }
            : {
                status: outcome.status === "cancelled" ? "stopped" : "failed",
                note: outcome.error,
              },
        );
        continue;
      }
      earlier.push({ name: member.name, text: stream.spoken.join(" ") });
    }
    setThinkingId(undefined);
    await player.current?.drained();
    if (round.current !== id) return;
    setSpeakingId(undefined);
    setPhase("listening");
  };
  heard.current = (text) => {
    if (live.current && text.trim()) void runRound(text);
  };

  /** Drops the round in progress: its audio and the answers still coming. */
  const dropRound = () => {
    round.current++;
    player.current?.stop();
    for (const stream of streams.current.values())
      optionsRef.current.stop?.(stream.sessionId);
    streams.current.clear();
    setThinkingId(undefined);
    setSpeakingId(undefined);
    editLines((current) =>
      current.map((line) =>
        line.status === "thinking" ? { ...line, status: "stopped" } : line,
      ),
    );
  };

  const begin = async () => {
    const ids = optionsRef.current.members.map((member) => member.id);
    voices.current = memberVoices(ids, await voiceSpeakers());
    live.current = true;
    startedAt.current = Date.now();
    setPhase("listening");
  };

  const start = useCallback(() => {
    if (live.current) return;
    setError(undefined);
    linesRef.current = [];
    setLines([]);
    player.current?.close();
    player.current = new CallPlayer((reason) => setError(reasonText(reason)));
    setPhase("starting");
    void voiceStatus()
      .then((status) => {
        setVoiceBytes(status.bytes);
        if (!status.installed) return setPhase("needs-voice");
        return begin();
      })
      .catch((reason) => {
        setError(reasonText(reason));
        setPhase("idle");
      });
  }, []);

  const installVoices = useCallback(() => {
    setError(undefined);
    setVoiceProgress(0);
    setPhase("installing-voice");
    void (async () => {
      const stop = await onVoiceInstall(({ done, total }) =>
        setVoiceProgress(total ? done / total : 0),
      );
      try {
        await installVoice();
        stop();
        await begin();
      } catch (reason) {
        stop();
        setError(reasonText(reason));
        setPhase("idle");
      }
    })();
  }, []);

  const finish = useCallback((save: boolean) => {
    const wasLive = live.current;
    live.current = false;
    dropRound();
    player.current?.close();
    player.current = null;
    const mic = dictationRef.current;
    if (mic.phase === "listening" || mic.phase === "starting") mic.cancel();
    const transcript = callTranscript(
      optionsRef.current.group.name,
      startedAt.current,
      linesRef.current,
    );
    if (save && wasLive && transcript)
      void Promise.resolve(
        optionsRef.current.save?.(transcript.title, transcript.body),
      ).catch(() => undefined);
  }, []);

  const hangUp = useCallback(() => {
    finish(true);
    setPhase("idle");
  }, [finish]);

  // Leaving the group hangs up, keeping what was said.
  useEffect(() => () => finish(true), [finish]);

  const interrupt = useCallback(() => {
    if (!live.current) return;
    dropRound();
    setPhase("listening");
  }, []);

  const say = useCallback((text: string) => {
    if (!live.current || !text.trim()) return;
    const mic = dictationRef.current;
    if (mic.phase === "listening" || mic.phase === "starting") mic.cancel();
    dropRound();
    void runRound(text);
  }, []);

  const endTurn = useCallback(() => {
    const mic = dictationRef.current;
    if (live.current && mic.phase === "listening") mic.toggle();
  }, []);

  const toggleMute = useCallback(() => setMuted((value) => !value), []);
  const dismiss = useCallback(() => {
    setError(undefined);
    dictationRef.current.dismiss();
  }, []);

  // The user's turn: open the mic once, unless muted or it needs attention
  // (the model download, an error). Levels re-render often; these effects
  // only follow the fields they act on.
  const opening = useRef(false);
  useEffect(() => {
    if (dictation.phase !== "idle" || dictation.error) opening.current = false;
  }, [dictation.phase, dictation.error]);
  useEffect(() => {
    if (phase !== "listening") return;
    const mic = dictationRef.current;
    if (muted) {
      if (mic.phase === "listening" || mic.phase === "starting") mic.cancel();
      return;
    }
    if (mic.phase === "idle" && !mic.error && !opening.current) {
      opening.current = true;
      mic.toggle();
    }
  }, [phase, muted, dictation.phase, dictation.error]);

  // A pause after something was heard passes the turn to the members.
  useEffect(() => {
    if (phase !== "listening" || dictation.phase !== "listening") return;
    if (dictation.speaking || !dictation.partial.trim()) return;
    const timer = setTimeout(
      () => dictationRef.current.toggle(),
      CALL_TURN_GAP_MS,
    );
    return () => clearTimeout(timer);
  }, [phase, dictation.phase, dictation.speaking, dictation.partial]);

  return {
    phase,
    lines,
    speakingId,
    thinkingId,
    muted,
    voiceBytes,
    voiceProgress,
    error,
    dictation,
    start,
    installVoices,
    hangUp,
    interrupt,
    endTurn,
    toggleMute,
    say,
    dismiss,
  };
}
