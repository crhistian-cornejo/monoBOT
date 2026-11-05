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
