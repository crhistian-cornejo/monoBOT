import { useMemo, useSyncExternalStore } from "react";
import { Call, CallEnd, Mic, MicOff, Square } from "../../../shared/ui/icons";
import { DictationBar } from "../../dictation/ui/Dictation";
import { formatModelSize } from "../../dictation/model/dictation";
import { MonoComposer } from "../../monos/ui/MonoComposer";
import { MonoRailMascot } from "../../monos/ui/MonoRailMascot";
import {
  monoLook,
  monosSnapshot,
  subscribeMonos,
  type Mono,
} from "../../monos/model/mono";
import type { Session } from "../../sessions/model/session";
import type { CallLine } from "../model/groupCall";
import { groupMembers } from "../model/groupBrief";
import {
  useGroupCall,
  type GroupCallOptions,
  type GroupCallState,
} from "./useGroupCall";

type Props = {
  /** The group's Mono. */
  group: Mono;
  sessions: readonly Session[];
  onAskMember: GroupCallOptions["ask"];
  onStopMember?: GroupCallOptions["stop"];
  onSaveTranscript?: GroupCallOptions["save"];
};

const BUTTON =
  "flex h-7 items-center gap-1.5 border border-stroke px-2.5 text-xs hover:bg-content/5 disabled:opacity-50";

/**
 * The group's Call tab: the user talks with the members out loud, and each
 * one answers in its own voice. Speech is heard and spoken on this computer;
 * only the words reach the members, as in the group's chat.
 */
export function GroupCall({
  group,
  sessions,
  onAskMember,
  onStopMember,
  onSaveTranscript,
}: Props) {
  const snapshot = useSyncExternalStore(
    subscribeMonos,
    monosSnapshot,
    monosSnapshot,
  );
  const monos = useMemo(
    () => (group.group ? groupMembers(group.group) : []),
    // The roster lives in the monos store; the snapshot says when it changed.
    [group.group, snapshot],
  );
  const members = useMemo(
    () => monos.map((mono) => ({ id: mono.id, name: monoLook(mono).name })),
    [monos],
  );
  const look = monoLook(group);
  const call = useGroupCall({
    group: { id: group.id, name: look.name, goal: group.group?.goal },
    members,
    sessions,
    ask: onAskMember,
    stop: onStopMember,
    save: onSaveTranscript,
  });
  const live = call.phase === "listening" || call.phase === "answering";
  const nameOf = (id?: string) =>
    members.find((member) => member.id === id)?.name;

  return (
    <div className="flex flex-col gap-3" data-group-call={call.phase}>
      <p className="px-2 text-[13px] text-content/70" aria-live="polite">
        {status(call, nameOf)}
      </p>
      {monos.length ? (
        <div className="flex flex-col border border-stroke">
          {monos.map((mono) => (
            <MemberTile
              key={mono.id}
              mono={mono}
              state={
                call.speakingId === mono.id
                  ? "speaking"
                  : call.thinkingId === mono.id
                    ? "thinking"
                    : "idle"
              }
            />
          ))}
        </div>
      ) : (
        <p className="px-2 text-xs text-content/50">
          Add members on the Members tab to call them.
        </p>
      )}
      {call.lines.length ? <Transcript lines={call.lines} /> : null}
      {call.error ? (
        <div className="flex items-center gap-2 border border-stroke px-3 py-1.5 text-xs">
          <span className="min-w-0 flex-1 text-red-400">{call.error}</span>
          <button
            type="button"
            onClick={call.dismiss}
            className="text-content/55 hover:text-content"
          >
            Dismiss
          </button>
        </div>
      ) : null}
      <VoiceDownload call={call} />
      {live ? (
        <div className="border border-stroke empty:hidden [&>div]:border-t-0">
          <DictationBar dictation={call.dictation} />
        </div>
      ) : null}
      <div className="flex flex-wrap items-center gap-1.5 px-2">
        {live ? (
          <>
            <button
              type="button"
              onClick={call.toggleMute}
              aria-pressed={call.muted}
              className={BUTTON}
            >
              {call.muted ? (
                <MicOff className="size-3.5" />
              ) : (
                <Mic className="size-3.5" />
              )}
              {call.muted ? "Unmute" : "Mute"}
            </button>
            {call.phase === "answering" ? (
              <button type="button" onClick={call.interrupt} className={BUTTON}>
                <Square className="size-2.5 fill-current" strokeWidth={0} />
                Interrupt
              </button>
            ) : (
              <button
                type="button"
                onClick={call.endTurn}
                disabled={
                  call.dictation.phase !== "listening" ||
                  !call.dictation.partial.trim()
                }
                className={BUTTON}
              >
                Done talking
              </button>
            )}
            <button
              type="button"
              onClick={call.hangUp}
              className="ml-auto flex h-7 items-center gap-1.5 bg-red-500 px-2.5 text-xs font-medium text-white hover:bg-red-500/90"
            >
              <CallEnd className="size-3.5" />
              Hang up
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={call.start}
            disabled={
              !members.length ||
              call.phase === "starting" ||
              call.phase === "installing-voice"
            }
            className="flex h-7 items-center gap-1.5 bg-accent px-2.5 text-xs font-medium text-white disabled:opacity-50"
          >
            <Call className="size-3.5" />
            Start call
          </button>
        )}
      </div>
      {live ? (
        <MonoComposer
          sessionId={`group-call-${group.id}`}
          name={look.name}
          dictation={false}
          onSubmit={(text) => {
            if (!text.trim()) return false;
            call.say(text);
          }}
        />
      ) : null}
    </div>
  );
}

function status(
  call: GroupCallState,
  nameOf: (id?: string) => string | undefined,
): string {
  switch (call.phase) {
    case "idle":
    case "needs-voice":
      return "Talk with the group out loud. Each member answers in its own voice, one after another; name someone to hear just them.";
    case "starting":
      return "Getting the voices ready…";
    case "installing-voice":
      return "Downloading the voices…";
    case "listening":
      return call.muted
        ? "Muted. Unmute to talk, or type below."
        : "Your turn. Talk, then pause; the members answer when you stop.";
    case "answering": {
      const speaking = nameOf(call.speakingId);
      const thinking = nameOf(call.thinkingId);
      return speaking
        ? `${speaking} is speaking.`
        : thinking
          ? `${thinking} is thinking…`
          : "The members are answering…";
    }
  }
}

function MemberTile({
  mono,
  state,
}: {
  mono: Mono;
  state: "speaking" | "thinking" | "idle";
}) {
  const look = monoLook(mono);
  return (
    <div
      data-call-member={state}
      className={`flex items-center gap-2 px-2 py-1.5 ${state === "speaking" ? "bg-accent/10" : ""}`}
    >
      <MonoRailMascot
        name={look.mascot}
        color={look.color}
        status={state === "idle" ? "idle" : "working"}
      />
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium">
        {look.name}
      </span>
      <span className="shrink-0 text-xs text-content/50">
        {state === "speaking"
          ? "Speaking"
          : state === "thinking"
            ? "Thinking…"
            : ""}
      </span>
    </div>
  );
}

const LINE_NOTE: Record<CallLine["status"], string> = {
  said: "",
  thinking: "Thinking…",
  passed: "Had nothing to add.",
  busy: "Busy right now.",
  failed: "Could not answer.",
  stopped: "Stopped.",
};

function Transcript({ lines }: { lines: readonly CallLine[] }) {
  return (
    <ol className="flex flex-col gap-1.5 px-2" data-call-transcript>
      {lines.map((line) => (
        <li key={line.id} className="text-[13px] leading-5">
          <span className="font-medium">{line.name}</span>{" "}
          {line.text ? (
            <span className="text-content/80">{line.text}</span>
          ) : (
            <span className="text-content/45" title={line.note}>
              {LINE_NOTE[line.status]}
            </span>
          )}
        </li>
      ))}
    </ol>
  );
}

/** The voices' one-time download, as dictation offers its model. */
function VoiceDownload({ call }: { call: GroupCallState }) {
  if (call.phase === "needs-voice")
    return (
      <div className="flex items-center gap-2 border border-stroke px-3 py-1.5 text-xs">
        <span className="min-w-0 flex-1 text-content/70">
          Calls speak with voices that run on this computer. Download them once
          ({formatModelSize(call.voiceBytes)}, Spanish and English) to start.
        </span>
        <button
          type="button"
          onClick={call.installVoices}
          className="bg-accent px-2 py-0.5 font-medium text-white"
        >
          Download
        </button>
      </div>
    );
  if (call.phase !== "installing-voice") return null;
  const percent = Math.round(call.voiceProgress * 100);
  return (
    <div className="flex items-center gap-3 border border-stroke px-3 py-1.5 text-xs text-content/70">
      <span className="shrink-0">Downloading the voices…</span>
      <span className="relative h-1 min-w-0 flex-1 bg-content/10">
        <span
          className="absolute inset-y-0 left-0 bg-accent"
          style={{ width: `${percent}%` }}
        />
      </span>
      <span className="shrink-0 tabular-nums">{percent}%</span>
    </div>
  );
}
