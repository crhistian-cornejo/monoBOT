import { useEffect, useState, type ReactNode } from "react";
import { AgentMarkdown } from "../../sessions/ui/AgentMarkdown";
import { openPaper } from "../../papers/model/openPaper";
import { paperKnown } from "../../papers/model/paperStore";
import { PaperReader } from "../../papers/ui/PaperReader";
import { HtmlArtifactFrame } from "./HtmlArtifactFrame";
import {
  ARTIFACTS_CHANGED_EVENT,
  getArtifact,
  type Artifact,
} from "../artifacts";

/** Load an artifact and follow later saves, for every reader frame. */
export function useArtifact(id: string) {
  const [artifact, setArtifact] = useState<Artifact | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    let request = 0;
    setArtifact(null);
    setLoaded(false);
    setError(null);
    const refresh = () => {
      const token = ++request;
      void getArtifact(id).then(
        (saved) => {
          if (!live || token !== request) return;
          setArtifact(saved);
          setError(null);
          setLoaded(true);
        },
        () => {
          if (!live || token !== request) return;
          setError("Could not load this document.");
          setLoaded(true);
        },
      );
    };
    refresh();
    window.addEventListener(ARTIFACTS_CHANGED_EVENT, refresh);
    window.addEventListener("focus", refresh);
    return () => {
      live = false;
      window.removeEventListener(ARTIFACTS_CHANGED_EVENT, refresh);
      window.removeEventListener("focus", refresh);
    };
  }, [id]);
  return { artifact, loaded, error, setError };
}

const PAPER_MARKER = /^\[monobot-paper\]:\s*paper:(\S+)\s*$/m;

/** The cached paper a document written by `papers.read` stands for. */
export function paperKeyOfDocument(body: string): string | undefined {
  const match = PAPER_MARKER.exec(body);
  if (!match) return undefined;
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return undefined;
  }
}

/**
 * A paper's page in Documents: the paper itself, with what answers cited from
 * it underlined. A paper the app no longer holds shows the saved text instead.
 */
function PaperDossier({
  paperKey,
  fallback,
  fill,
}: {
  paperKey: string;
  fallback: ReactNode;
  /** The reader's frame is a column of the panel's height: take all of it. */
  fill?: boolean;
}) {
  const [held, setHeld] = useState<boolean>();
  useEffect(() => {
    let live = true;
    setHeld(undefined);
    void paperKnown(paperKey).then((known) => live && setHeld(known));
    return () => {
      live = false;
    };
  }, [paperKey]);
  if (held === undefined) return null;
  if (!held)
    return fill ? (
      <div className="min-h-0 flex-1 overflow-y-auto">{fallback}</div>
    ) : (
      <>{fallback}</>
    );
  return (
    <div
      className={`overflow-hidden rounded-lg border border-content/10 ${
        fill ? "min-h-0 flex-1" : "-mx-1 h-[72vh] min-h-[420px]"
      }`}
    >
      <PaperReader
        paperKey={paperKey}
        embedded
        onExpand={() => openPaper({ key: paperKey })}
      />
    </div>
  );
}

/** Each kind owns its presentation inside the shared reader frame. */
export function ArtifactContent({
  artifact,
  onOpenFile,
  fill,
}: {
  artifact: Artifact;
  onOpenFile?: (path: string) => void;
  /** Inside a frame as tall as the panel: a paper fills it instead of a fixed height. */
  fill?: boolean;
}) {
  switch (artifact.kind) {
    case "document": {
      const paper = paperKeyOfDocument(artifact.body);
      return paper ? (
        <PaperDossier
          fill={fill}
          paperKey={paper}
          fallback={
            <AgentMarkdown
              text={artifact.body}
              cwd={artifact.sourceCwd}
              onOpenFile={onOpenFile}
              hardBreaks
              citations="chips"
            />
          }
        />
      ) : (
        <AgentMarkdown
          text={artifact.body}
          cwd={artifact.sourceCwd}
          onOpenFile={onOpenFile}
          hardBreaks
          citations="chips"
        />
      );
    }
    case "html":
      return (
        <HtmlArtifactFrame
          id={artifact.id}
          title={artifact.title}
          version={artifact.updatedAt}
        />
      );
  }
}
