import { useState, useSyncExternalStore } from "react";
import { FileCard, fileFormatOf } from "../../../shared/ui/FileCard";
import { ChevronDown, Folder, FolderOpen, Star } from "../../../shared/ui/icons";
import { artifactLabel, type ArtifactCard } from "../artifacts";
import {
  documentStarsSnapshot,
  groupDay,
  subscribeDocumentStars,
  toggleDocumentStar,
  type DocumentGroup,
} from "../documentGroups";
import { cardFormat } from "./ArtifactCard";

/**
 * A chat's documents as folders, one per request and under its day: the
 * final report on top, the papers it read below. A star keeps a document at
 * hand; "Starred" shows only those.
 */
export function DocumentFolders({
  groups,
  onOpen,
  onOpenFile,
}: {
  groups: readonly DocumentGroup[];
  onOpen: (id: string) => void;
  /** Opens a file the bot wrote in the app's viewer. */
  onOpenFile?: (path: string) => void;
}) {
  const stars = useSyncExternalStore(
    subscribeDocumentStars,
    documentStarsSnapshot,
  );
  const [starredOnly, setStarredOnly] = useState(false);
  const anyStarred = groups.some((group) =>
    group.cards.some((card) => stars.has(card.id)),
  );
  const onlyStars = starredOnly && anyStarred;
  const shown = onlyStars
    ? groups
        .map((group) => ({
          ...group,
          cards: group.cards.filter((card) => stars.has(card.id)),
          files: [],
        }))
        .filter((group) => group.cards.length)
    : groups;

  let lastDay: string | null = null;
  return (
    <div className="flex flex-col gap-1" data-document-folders>
      {anyStarred ? (
        <div className="flex gap-1 px-1 pb-1 text-[12px]">
          {[
            { id: false, label: "All" },
            { id: true, label: "Starred" },
          ].map((option) => (
            <button
              key={option.label}
              type="button"
              aria-pressed={onlyStars === option.id}
              onClick={() => setStarredOnly(option.id)}
              className={`rounded-md px-2 py-1 ${
                onlyStars === option.id
                  ? "bg-content/8 text-content"
                  : "text-content/55 hover:bg-content/5 hover:text-content"
              }`}
            >
              {option.label}
            </button>
          ))}
        </div>
      ) : null}
      {shown.map((group, index) => {
        const day = groupDay(group.at);
        const header = day !== lastDay ? day : null;
        lastDay = day;
        return (
          <div key={group.id}>
            {header ? (
              <h3 className="px-2 pt-2 pb-1 text-[11px] font-medium uppercase tracking-wide text-content/40">
                {header}
              </h3>
            ) : null}
            <RequestFolder
              group={group}
              stars={stars}
              // The newest request opens; older ones stay folded.
              open={index === 0 || onlyStars}
              onOpen={onOpen}
              onOpenFile={onOpenFile}
            />
          </div>
        );
      })}
    </div>
  );
}

function RequestFolder({
  group,
  stars,
  open: initiallyOpen,
  onOpen,
  onOpenFile,
}: {
  group: DocumentGroup;
  stars: ReadonlySet<string>;
  open: boolean;
  onOpen: (id: string) => void;
  onOpenFile?: (path: string) => void;
}) {
  const count = group.cards.length + group.files.length;
  const [open, setOpen] = useState(initiallyOpen);
  const Icon = open ? FolderOpen : Folder;
  return (
    <section data-document-folder={group.id}>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        title={group.title}
        className="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-content/5"
      >
        <Icon className="size-4 shrink-0 text-content/50" strokeWidth={1.75} />
        <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-content/80">
          {group.title}
        </span>
        {group.at !== undefined ? (
          <span className="shrink-0 text-[11px] tabular-nums text-content/40">
            {new Date(group.at).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </span>
        ) : null}
        <span
          aria-label={`${count} documents and files`}
          className="grid h-4 min-w-4 shrink-0 place-items-center rounded-full bg-content/8 px-1 text-[10px] tabular-nums text-content/55"
        >
          {count}
        </span>
        <ChevronDown
          aria-hidden
          className={`size-3.5 shrink-0 text-content/40 transition-transform ${
            open ? "rotate-180" : ""
          }`}
        />
      </button>
      {open ? (
        <ul className="ml-4 flex flex-col gap-px border-l border-content/8 pl-1.5">
          {group.cards.map((card) => (
            <li key={card.id}>
              <DocumentRow
                card={card}
                starred={stars.has(card.id)}
                onOpen={onOpen}
              />
            </li>
          ))}
          {group.files.map((path) => (
            <li key={path}>
              <FileRow path={path} onOpen={onOpenFile} />
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

/** A file the bot wrote, by its name and folder; opens in the app's viewer. */
function FileRow({
  path,
  onOpen,
}: {
  path: string;
  onOpen?: (path: string) => void;
}) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  const name = parts.pop() ?? path;
  const folder = parts.slice(-2).join("/");
  return (
    <button
      type="button"
      data-document-file={path}
      title={path}
      aria-label={`Open ${name}`}
      onClick={() => onOpen?.(path)}
      disabled={!onOpen}
      className="flex w-full min-w-0 items-center gap-2.5 rounded-md px-2 py-2 text-left hover:bg-content/5 disabled:opacity-60"
    >
      <FileCard format={fileFormatOf(name)} size="sm" />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[12px] leading-5 text-content/85">
          {name}
        </span>
        <span className="truncate text-[11px] leading-4 text-content/40">
          {folder || "File"}
        </span>
      </span>
    </button>
  );
}

function DocumentRow({
  card,
  starred,
  onOpen,
}: {
  card: ArtifactCard;
  starred: boolean;
  onOpen: (id: string) => void;
}) {
  const label = card.final ? "Final report" : artifactLabel(card.kind);
  return (
    <div
      className={`group flex min-w-0 items-center rounded-md ${
        card.final ? "bg-accent/8 hover:bg-accent/12" : "hover:bg-content/5"
      }`}
    >
      <button
        type="button"
        data-mono-document={card.id}
        aria-label={`Open ${label.toLowerCase()} ${card.title}`}
        onClick={() => onOpen(card.id)}
        className="flex min-w-0 flex-1 items-center gap-2.5 px-2 py-2 text-left"
      >
        <FileCard format={cardFormat(card)} size="sm" />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[12px] leading-5 text-content/85">
            {card.title}
          </span>
          <span
            className={`truncate text-[11px] leading-4 ${
              card.final ? "text-accent" : "text-content/40"
            }`}
          >
            {card.final ? label : card.summary || label}
          </span>
        </span>
      </button>
      <button
        type="button"
        aria-label={starred ? "Unstar" : "Star"}
        aria-pressed={starred}
        title={starred ? "Unstar" : "Star to find it again"}
        onClick={() => toggleDocumentStar(card.id)}
        className={`mr-1 grid size-7 shrink-0 place-items-center rounded-md hover:bg-content/8 ${
          starred
            ? "text-accent"
            : "text-content/40 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
        }`}
      >
        <Star
          className="size-3.5"
          strokeWidth={1.75}
          fill={starred ? "currentColor" : "none"}
        />
      </button>
    </div>
  );
}
