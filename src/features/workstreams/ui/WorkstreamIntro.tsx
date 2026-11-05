import { useState, type ReactNode } from "react";
import { BetaBadge } from "../../../shared/ui/BetaBadge";
import { Modal } from "../../../shared/ui/Modal";

function Node({
  children,
  lead = false,
}: {
  children: ReactNode;
  lead?: boolean;
}) {
  return (
    <span
      className={`rounded-md border px-2.5 py-1 text-[12px] ${
        lead
          ? "border-accent/40 bg-accent/10 text-accent"
          : "border-content/10 bg-background-base text-content/70"
      }`}
    >
      {children}
    </span>
  );
}

/** One node handing work to several. */
function Fan({ from, to }: { from: ReactNode; to: ReactNode[] }) {
  return (
    <div className="flex items-center justify-center gap-3">
      {from}
      <span aria-hidden className="h-px w-6 bg-content/15" />
      <div className="flex flex-col gap-1.5 border-l border-content/15 pl-3">
        {to}
      </div>
    </div>
  );
}

const STEPS: { title: string; body: string; diagram: ReactNode }[] = [
  {
    title: "A goal, not a single task",
    body: "Give a workstream something that spans several parts of a repository: a migration, a feature across front end and back end, a batch of bugs. An orchestrator plans it with you and runs one agent per part at the same time.",
    diagram: (
      <Fan
        from={<Node lead>Orchestrator</Node>}
        to={[
          <Node key="a">Agent · API</Node>,
          <Node key="w">Agent · Web</Node>,
          <Node key="t">Agent · Tests</Node>,
        ]}
      />
    ),
  },
  {
    title: "Each agent on its own branch",
    body: "Every agent works in its own worktree on the branch ws/<name>/<part>, so agents never touch each other's files. When an agent finishes, it opens a draft pull request and reports back to the orchestrator.",
    diagram: (
      <Fan
        from={<Node lead>Repository</Node>}
        to={[
          <Node key="1">ws/…/api</Node>,
          <Node key="2">ws/…/web</Node>,
          <Node key="3">ws/…/tests</Node>,
        ]}
      />
    ),
  },
  {
    title: "You decide, it coordinates",
    body: "Talk to the orchestrator in its chat. It writes the plan to the workstream's shared folder, asks you only for the decisions that are yours, and applies the rules you set to every agent.",
    diagram: (
      <div className="flex flex-col items-center gap-1.5">
        <Node>You</Node>
        <span aria-hidden className="h-4 w-px bg-content/15" />
        <Node lead>Orchestrator</Node>
        <span aria-hidden className="h-4 w-px bg-content/15" />
        <div className="flex gap-1.5">
          <Node>Agent</Node>
          <Node>Agent</Node>
          <Node>Agent</Node>
        </div>
      </div>
    ),
  },
  {
    title: "Review what comes back",
    body: "Follow each agent in Agents, read plans and notes in Files, and review the work in Pull requests. Agents run on this computer and stop when you quit MonoBOT. A workstream uses far more tokens than one session, so keep it for large work.",
    diagram: (
      <Fan
        from={<Node lead>Workstream</Node>}
        to={[
          <Node key="a">Agents</Node>,
          <Node key="f">Files</Node>,
          <Node key="p">Pull requests</Node>,
        ]}
      />
    ),
  },
];

/** The first-run tour, shown once before the first workstream. */
export function WorkstreamIntro({
  onStart,
  onClose,
}: {
  onStart: () => void;
  onClose: () => void;
}) {
  const [step, setStep] = useState(0);
  const current = STEPS[step]!;
  const last = step === STEPS.length - 1;
  return (
    <Modal title={current.title} onClose={onClose} size="md" minimalHeader>
      <div className="flex min-h-48 items-center justify-center border-b border-content/7 bg-content/[0.03] px-5 py-8">
        {current.diagram}
      </div>
      <div className="flex flex-col gap-1.5 px-5 pt-4">
        <div className="flex items-center gap-2 text-[12px] text-content/50">
          <span>Workstreams</span>
          <BetaBadge />
          <span className="ml-auto tabular-nums">
            {step + 1} of {STEPS.length}
          </span>
        </div>
        <h2 className="text-md font-semibold tracking-tight text-content">
          {current.title}
        </h2>
        <p className="text-[13px] leading-relaxed text-content/65">
          {current.body}
        </p>
      </div>
      <div className="flex items-center gap-2 px-5 pb-4 pt-5">
        <div className="flex gap-1.5" aria-hidden>
          {STEPS.map((_, index) => (
            <span
              key={index}
              className={`size-1.5 rounded-full ${
                index === step ? "bg-accent" : "bg-content/20"
              }`}
            />
          ))}
        </div>
        <button
          type="button"
          onClick={onClose}
          className="ml-auto rounded-md px-3 py-1.5 text-[12px] text-content/60 hover:bg-content/8 hover:text-content active:scale-[0.97]"
        >
          Skip
        </button>
        {step > 0 ? (
          <button
            type="button"
            onClick={() => setStep(step - 1)}
            className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8 active:scale-[0.97]"
          >
            Back
          </button>
        ) : null}
        <button
          type="button"
          onClick={() => (last ? onStart() : setStep(step + 1))}
          className="primary-action rounded-md px-3 py-1.5 text-[12px] font-medium active:scale-[0.97]"
        >
          {last ? "Start a workstream" : "Next"}
        </button>
      </div>
    </Modal>
  );
}
