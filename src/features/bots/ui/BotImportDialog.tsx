import { useEffect, useMemo, useState } from "react";
import { Modal } from "../../../shared/ui/Modal";
import { homeDir } from "../../../platform/tauri/fs";
import {
  canRunHarnessTextPrompt,
  runHarnessTextPrompt,
} from "../../../integrations/harness/core/registry";
import { pickTextHarness } from "../../../integrations/harness/core/textHarness";
import { MonoRailMascot } from "../../monos/ui/MonoRailMascot";
import { findMono, listMonos, monoLook } from "../../monos/model/mono";
import {
  compareBots,
  comparisonPrompt,
  existingMatch,
  type BotComparison,
  type BotExport,
  type ComparisonRow,
  type ExportedBot,
} from "../model/botTransfer";
import {
  createImportedBot,
  replaceBot,
  snapshotBot,
} from "../model/botTransferIO";

export type Choice = "keep" | "replace" | "both";

type Conflict = {
  incoming: ExportedBot;
  existingId: string;
  current?: ExportedBot;
  comparison?: BotComparison;
  /** A model's own reading of the two; undefined while it thinks. */
  opinion?: string | null;
  choice: Choice;
};

const PLATFORM_NAMES: Record<string, string> = {
  windows: "Windows",
  macos: "Mac",
  linux: "Linux",
};

/** Imports a bots file: new bots are added; one that exists is compared first. */
export function BotImportDialog({
  file,
  matches,
  onClose,
  onDone,
  onDecided,
}: {
  file: BotExport;
  /** The bot here each incoming one is, when already known (the sync pairs them by id). */
  matches?: readonly (string | undefined)[];
  onClose: () => void;
  onDone: (summary: string) => void;
  /** What the user chose for each bot that was already here. */
  onDecided?: (choices: { bot: ExportedBot; choice: Choice }[]) => void;
}) {
  const plan = useMemo(() => {
    const roster = listMonos()
      .filter((mono) => mono.kind === "bot")
      .map((mono) => ({
        id: mono.id,
        name: monoLook(mono).name,
        botRole: mono.botRole,
      }));
    const fresh: ExportedBot[] = [];
    const conflicts: Conflict[] = [];
    for (const [index, incoming] of file.bots.entries()) {
      const known = matches?.[index];
      const match = known
        ? roster.find((bot) => bot.id === known)
        : existingMatch(incoming, roster);
      if (match) conflicts.push({ incoming, existingId: match.id, choice: "keep" });
      else fresh.push(incoming);
    }
    return { fresh, conflicts };
  }, [file, matches]);
  const [conflicts, setConflicts] = useState(plan.conflicts);
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  // Read each bot here and compare it; the suggestion becomes the default choice.
  useEffect(() => {
    let alive = true;
    plan.conflicts.forEach((conflict, index) => {
      const mono = findMono(conflict.existingId);
      if (!mono) return;
      void snapshotBot(mono).then((current) => {
        if (!alive) return;
        const comparison = compareBots(current, conflict.incoming);
        setConflicts((list) =>
          list.map((entry, at) =>
            at === index
              ? {
                  ...entry,
                  current,
                  comparison,
                  choice:
                    comparison.suggestion.keep === "incoming" ? "replace" : "keep",
                }
              : entry,
          ),
        );
        void secondOpinion(current, conflict.incoming).then((opinion) => {
          if (!alive) return;
          setConflicts((list) =>
            list.map((entry, at) => (at === index ? { ...entry, opinion } : entry)),
          );
        });
      });
    });
    return () => {
      alive = false;
    };
  }, [plan]);

  const conflict = conflicts[page];
  const from = PLATFORM_NAMES[file.platform] ?? "another computer";
  const exported = file.exportedAt
    ? new Date(file.exportedAt).toLocaleDateString(undefined, {
        dateStyle: "medium",
      })
    : "";

  const apply = async () => {
    setBusy(true);
    setError(undefined);
    try {
      for (const bot of plan.fresh) await createImportedBot(bot);
      let replaced = 0;
      let copies = 0;
      for (const entry of conflicts) {
        if (entry.choice === "replace") {
          await replaceBot(entry.existingId, entry.incoming);
          replaced++;
        } else if (entry.choice === "both") {
          await createImportedBot(entry.incoming, true);
          copies++;
        }
      }
      const parts = [
        plan.fresh.length && `${plan.fresh.length} added`,
        replaced && `${replaced} replaced`,
        copies && `${copies} kept as a copy`,
      ].filter(Boolean);
      onDecided?.(conflicts.map((entry) => ({ bot: entry.incoming, choice: entry.choice })));
      onDone(parts.length ? `Bots imported: ${parts.join(", ")}.` : "No bots changed.");
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      setBusy(false);
    }
  };

  return (
    <Modal
      title="Import bots"
      description={`${file.bots.length} bot${file.bots.length === 1 ? "" : "s"} from ${from}${exported ? `, exported ${exported}` : ""}.`}
      size="lg"
      onClose={busy ? () => undefined : onClose}
    >
      <div className="flex max-h-[70vh] flex-col gap-4 overflow-y-auto px-5 py-4 text-[13px]">
        {plan.fresh.length ? (
          <section>
            <h3 className="mb-1.5 text-[12px] text-content/70">New, added as they are</h3>
            <ul className="flex flex-wrap gap-1.5">
              {plan.fresh.map((bot) => (
                <li
                  key={bot.name}
                  className="flex items-center gap-1.5 rounded-md border border-content/10 px-2 py-1"
                >
                  <MonoRailMascot name={bot.mascot} color={bot.color} />
                  {bot.name}
                </li>
              ))}
            </ul>
          </section>
        ) : null}

        {conflict ? (
          <section className="flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <h3 className="font-medium">
                “{conflict.incoming.name}” is already here
              </h3>
              {conflicts.length > 1 ? (
                <span className="text-[12px] text-content/50">
                  {page + 1} of {conflicts.length}
                </span>
              ) : null}
            </div>
            {conflict.comparison && conflict.current ? (
              <ConflictView conflict={conflict} from={from} />
            ) : (
              <p className="text-content/50">Reading the bot on this computer…</p>
            )}
            <div
              role="radiogroup"
              aria-label="What to keep"
              className="grid grid-cols-3 gap-2"
            >
              <ChoiceButton
                label="Keep this one"
                hint="The import skips it"
                active={conflict.choice === "keep"}
                onClick={() => choose(page, "keep")}
              />
              <ChoiceButton
                label="Replace it"
                hint="Its chat and folders stay"
                active={conflict.choice === "replace"}
                onClick={() => choose(page, "replace")}
              />
              <ChoiceButton
                label="Keep both"
                hint={
                  conflict.incoming.botRole === "principal"
                    ? "There is one orchestrator"
                    : "The imported one gets a new name"
                }
                active={conflict.choice === "both"}
                disabled={conflict.incoming.botRole === "principal"}
                onClick={() => choose(page, "both")}
              />
            </div>
          </section>
        ) : null}

        {error ? (
          <p role="alert" className="text-xs text-red-400">
            {error}
          </p>
        ) : null}
      </div>
      <div className="flex items-center justify-between gap-2 border-t border-stroke px-5 py-3">
        <div className="flex gap-1">
          {conflicts.length > 1 ? (
            <>
              <button
                type="button"
                disabled={page === 0}
                onClick={() => setPage(page - 1)}
                className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8 disabled:opacity-40"
              >
                Previous
              </button>
              <button
                type="button"
                disabled={page === conflicts.length - 1}
                onClick={() => setPage(page + 1)}
                className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8 disabled:opacity-40"
              >
                Next
              </button>
            </>
          ) : null}
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void apply()}
            className="primary-action rounded-md px-3 py-1.5 text-[12px]"
          >
            {busy ? "Importing…" : "Import"}
          </button>
        </div>
      </div>
    </Modal>
  );

  function choose(index: number, choice: Choice) {
    setConflicts((list) =>
      list.map((entry, at) => (at === index ? { ...entry, choice } : entry)),
    );
  }
}

function ConflictView({ conflict, from }: { conflict: Conflict; from: string }) {
  const { comparison, current, incoming } = {
    comparison: conflict.comparison!,
    current: conflict.current!,
    incoming: conflict.incoming,
  };
  return (
    <>
      <div className="grid grid-cols-2 gap-2">
        <SideCard title="On this computer" bot={current} summary={comparison.summary.current} />
        <SideCard title={`Imported from ${from}`} bot={incoming} summary={comparison.summary.incoming} />
      </div>
      <table className="w-full table-fixed border-collapse text-[12px]">
        <tbody>
          {comparison.rows.map((row) => (
            <ComparisonLine key={row.label} row={row} />
          ))}
        </tbody>
      </table>
      {comparison.changes.length ? (
        <div>
          <h4 className="mb-1 text-[12px] text-content/70">If you replace it</h4>
          <ul className="list-disc space-y-0.5 pl-5 text-[12px] text-content/80">
            {comparison.changes.map((change) => (
              <li key={change}>{change}</li>
            ))}
          </ul>
        </div>
      ) : null}
      <div className="rounded-md border border-accent/25 bg-accent/6 px-3 py-2 text-[12px]">
        <span className="font-medium text-accent">
          {comparison.suggestion.keep === "incoming"
            ? "Suggested: the imported one. "
            : comparison.suggestion.keep === "current"
              ? "Suggested: this one. "
              : "Either works. "}
        </span>
        <span className="text-content/75">{comparison.suggestion.why}</span>
        {conflict.opinion === undefined ? (
          <p className="mt-1.5 text-content/45">Asking a model for its reading…</p>
        ) : conflict.opinion ? (
          <div className="mt-1.5 whitespace-pre-wrap text-content/75">
            {conflict.opinion}
          </div>
        ) : null}
      </div>
    </>
  );
}

function SideCard({
  title,
  bot,
  summary,
}: {
  title: string;
  bot: ExportedBot;
  summary: string;
}) {
  return (
    <div className="rounded-md border border-content/10 px-3 py-2">
      <p className="text-[11px] text-content/50">{title}</p>
      <p className="mt-1 flex items-center gap-1.5 font-medium">
        <MonoRailMascot name={bot.mascot} color={bot.color} />
        {bot.name}
      </p>
      <p className="mt-1 text-[12px] text-content/65">{summary}</p>
    </div>
  );
}

function ComparisonLine({ row }: { row: ComparisonRow }) {
  const mark = (side: "current" | "incoming") =>
    row.verdict === side ? "text-content font-medium" : "text-content/65";
  return (
    <tr className="border-t border-content/8 align-top">
      <th scope="row" className="w-28 py-1.5 pr-2 text-left font-normal text-content/50">
        {row.label}
      </th>
      <td className={`break-words py-1.5 pr-2 ${mark("current")}`}>{row.current}</td>
      <td className={`break-words py-1.5 ${mark("incoming")}`}>
        {row.incoming}
        {row.verdict === "same" ? <span className="text-content/35"> · same</span> : null}
      </td>
    </tr>
  );
}

function ChoiceButton({
  label,
  hint,
  active,
  disabled,
  onClick,
}: {
  label: string;
  hint: string;
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={active}
      disabled={disabled}
      onClick={onClick}
      className={`rounded-md border px-3 py-2 text-left disabled:opacity-40 ${
        active
          ? "border-accent bg-accent/8"
          : "border-content/10 hover:bg-content/5"
      }`}
    >
      <span className="block text-[12px] font-medium">{label}</span>
      <span className="block text-[11px] text-content/50">{hint}</span>
    </button>
  );
}

/** A model's short reading of the two, when one can be asked; null otherwise. */
async function secondOpinion(
  current: ExportedBot,
  incoming: ExportedBot,
): Promise<string | null> {
  try {
    const harness = pickTextHarness();
    if (!canRunHarnessTextPrompt(harness)) return null;
    const text = await runHarnessTextPrompt({
      harness,
      cwd: await homeDir(),
      prompt: comparisonPrompt(current, incoming),
      timeoutMs: 60_000,
    });
    return text.trim() || null;
  } catch {
    return null;
  }
}
