import { FileCard, type FileFormat } from "../../../shared/ui/FileCard";
import { ChartBreakoutSquare, File } from "../../../shared/ui/icons";
import { artifactLabel, type ArtifactCard as Card } from "../artifacts";

export function ArtifactIcon({
  kind,
  className,
}: {
  kind: Card["kind"];
  className?: string;
}) {
  const Icon = kind === "html" ? ChartBreakoutSquare : File;
  return <Icon className={className} strokeWidth={1.75} />;
}

/** A paper's page in Documents is written by the paper tool under this prefix. */
export const isPaperCard = (id: string | undefined) =>
  !!id && id.startsWith("paper-");

/** The kind of file a card in the chat stands for: a paper is a PDF. */
export function cardFormat(card: Pick<Card, "id" | "kind">): FileFormat {
  if (isPaperCard(card.id)) return "pdf";
  return card.kind === "html" ? "html" : "md";
}

/**
 * One line under the reply: an artifact's icon and its title. The icon sets it
 * apart from the session cards beside it, which lead with their status; it
 * keeps their height, which their project chip sets.
 */
export function ArtifactCard({
  card,
  onOpen,
}: {
  card: Card;
  onOpen?: (id: string) => void;
}) {
  const label = artifactLabel(card.kind);
  if (card.final) return <FinalReportCard card={card} onOpen={onOpen} />;
  return (
    <button
      type="button"
      data-artifact-card={card.id}
      aria-label={`Open ${label.toLowerCase()} ${card.title}`}
      title={`Open ${label.toLowerCase()}`}
      onClick={() => onOpen?.(card.id)}
      disabled={!onOpen}
      className="flex w-fit max-w-[min(24rem,100%)] min-w-0 items-center gap-2.5 min-h-[38px] rounded-xl border border-content/10 py-2 pr-3.5 pl-3 text-left font-sans transition-colors hover:border-content/20 hover:bg-content/5 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:opacity-60"
    >
      <FileCard format={cardFormat(card)} size="sm" />
      <span className="min-w-0 truncate text-[13px] text-content">
        {card.title}
      </span>
    </button>
  );
}

/**
 * The report that closes a piece of work. It is larger and tinted with the
 * accent, so it reads as the answer and not one more paper gathered on the way.
 */
function FinalReportCard({
  card,
  onOpen,
}: {
  card: Card;
  onOpen?: (id: string) => void;
}) {
  return (
    <button
      type="button"
      data-artifact-card={card.id}
      data-final-report
      aria-label={`Open final report ${card.title}`}
      title="Open final report"
      onClick={() => onOpen?.(card.id)}
      disabled={!onOpen}
      className="flex w-full max-w-[min(28rem,100%)] min-w-0 items-center gap-3 rounded-xl border border-accent/30 bg-accent/8 py-3 pr-4 pl-3 text-left font-sans transition-colors hover:border-accent/45 hover:bg-accent/12 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:opacity-60"
    >
      <FileCard format={cardFormat(card)} size="sm" className="mx-1" />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-[13px] font-medium text-content">
          {card.title}
        </span>
        <span className="truncate text-[12px] text-content/55">
          <span className="font-medium text-accent">Final report</span>
          {card.summary ? ` · ${card.summary}` : " · Document"}
        </span>
      </span>
      <span className="shrink-0 rounded-md px-2 py-1 text-[12px] text-content/70">
        Open
      </span>
    </button>
  );
}
