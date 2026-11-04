import { useState, type ReactNode } from "react";
import { Modal } from "../../../shared/ui/Modal";

function Box({
  children,
  lead = false,
}: {
  children: ReactNode;
  lead?: boolean;
}) {
  return (
    <span
      className={`border px-2.5 py-1 font-mono text-[11px] uppercase tracking-wide ${
        lead ? "border-accent text-accent" : "border-stroke text-content/70"
      }`}
    >
      {children}
    </span>
  );
}

function Fan({ from, to }: { from: ReactNode; to: ReactNode[] }) {
  return (
    <div className="flex items-center justify-center gap-4">
      {from}
      <span aria-hidden className="h-px w-8 bg-accent" />
      <div className="flex flex-col gap-2 border-l border-accent pl-4">
        {to}
      </div>
    </div>
  );
}

const STEPS: { title: string; body: string; diagram: ReactNode }[] = [
  {
    title: "Delegate a project, not a task",
    body: "A workstream takes on work that would keep a team busy for days: a framework migration, a feature across several services, a backlog of bugs. Agents work on it in parallel and the work comes back as pull requests to review.",
    diagram: (
      <Fan
        from={<Box lead>Orchestrator</Box>}
        to={[
          <Box key="a">Agent · API</Box>,
          <Box key="w">Agent · Web</Box>,
          <Box key="t">Agent · Tests</Box>,
        ]}
      />
    ),
  },
  {
    title: "A lead and its team",
    body: "The orchestrator splits your goal into areas and gives each one to an agent. Agents build and report; the orchestrator keeps them coordinated and gathers the results. It does not write the code itself.",
    diagram: (
      <div className="flex flex-col items-center gap-2">
        <Box lead>Orchestrator</Box>
        <span className="font-mono text-[11px] text-content/50">
          assigns ↓ · reports ↑
        </span>
        <div className="flex gap-2">
          <Box>Agent</Box>
          <Box>Agent</Box>
          <Box>Agent</Box>
        </div>
      </div>
    ),
  },
  {
    title: "Each agent in its own worktree",
    body: "Agents work in separate git worktrees of your repository, so they never step on each other, and each one opens its own pull request. Local workstreams stop when you quit MonoBOT.",
    diagram: (
      <Fan
        from={<Box lead>Your repository</Box>}
        to={[
          <Box key="1">ws/…/api</Box>,
          <Box key="2">ws/…/web</Box>,
          <Box key="3">ws/…/tests</Box>,
        ]}
      />
    ),
  },
  {
    title: "You direct, you don't operate",
    body: "The workstream asks you only when a decision is yours, and the work comes back as pull requests. Follow any agent from the Agents tab. Workstreams use many more tokens than one session, so keep them for large work.",
    diagram: (
      <Fan
        from={<Box>You</Box>}
        to={[
          <Box key="o" lead>
            Orchestrator · chat
          </Box>,
          <Box key="a">Agents · answer</Box>,
          <Box key="p">PRs · review</Box>,
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
    <Modal title={current.title} onClose={onClose} size="lg" minimalHeader>
      <div className="flex flex-col">
        <div className="flex min-h-44 items-center justify-center border-b border-stroke py-8">
          {current.diagram}
        </div>
        <div className="flex flex-col gap-2 pt-4">
          <span className="font-mono text-[11px] uppercase tracking-wide text-accent">
            Workstreams · Beta · {step + 1} of {STEPS.length}
          </span>
          <h2 className="text-base font-semibold">{current.title}</h2>
          <p className="text-[13px] leading-relaxed text-content/70">
            {current.body}
          </p>
        </div>
        <div className="flex items-center gap-2 pt-5">
          <div className="flex gap-1.5" aria-hidden>
            {STEPS.map((_, index) => (
              <span
                key={index}
                className={`size-1.5 ${index === step ? "bg-accent" : "bg-content/25"}`}
              />
            ))}
          </div>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto h-8 px-3 text-xs text-content/60 hover:text-content"
          >
            Skip
          </button>
          {step > 0 ? (
            <button
              type="button"
              onClick={() => setStep(step - 1)}
              className="h-8 px-3 text-xs text-content/60 hover:text-content"
            >
              Back
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => (last ? onStart() : setStep(step + 1))}
            className="h-8 bg-accent px-3 text-xs font-medium text-white"
          >
            {last ? "Start a workstream" : "Next"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
