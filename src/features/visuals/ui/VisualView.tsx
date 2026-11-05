// One published visual in the chat. Its program runs in the app
// (src-tauri/src/visuals.rs) for as long as it is on screen, so its controls
// keep their state; reset starts it over.
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";

import { Code, RotateCcw } from "../../../shared/ui/icons";
import { AgentMarkdown } from "../../sessions/ui/AgentMarkdown";
import type { VisualDocument, VisualNode } from "../model/visual";
import { VisualTree } from "./VisualTree";

type Opened = { handle: number; tree: VisualNode };

const errorText = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export function VisualView({ document }: { document: VisualDocument }) {
  const [tree, setTree] = useState<VisualNode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [source, setSource] = useState(false);
  const [busy, setBusy] = useState(false);
  const [generation, setGeneration] = useState(0);
  const handle = useRef<number | null>(null);
  // Changes go one at a time, in order, each to the program the last left.
  const queue = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let live = true;
    setTree(null);
    setError(null);
    invoke<Opened>("visual_open", { source: document.source }).then(
      (opened) => {
        if (!live) {
          void invoke("visual_close", { handle: opened.handle });
          return;
        }
        handle.current = opened.handle;
        setTree(opened.tree);
      },
      (reason: unknown) => live && setError(errorText(reason)),
    );
    return () => {
      live = false;
      if (handle.current != null)
        void invoke("visual_close", { handle: handle.current });
      handle.current = null;
    };
  }, [document.source, generation]);

  const dispatch = useCallback((handler: string, value: unknown) => {
    queue.current = queue.current.then(async () => {
      const current = handle.current;
      if (current == null) return;
      setBusy(true);
      try {
        setTree(
          await invoke<VisualNode>("visual_dispatch", {
            handle: current,
            handler,
            value,
          }),
        );
      } catch (reason) {
        const message = errorText(reason);
        // The app closed this program to make room; start it over.
        if (message === "visual-expired") setGeneration((n) => n + 1);
        else if (!/stale/i.test(message)) setError(message);
      } finally {
        setBusy(false);
      }
    });
  }, []);

  const tool =
    "flex size-6 items-center justify-center rounded-md text-content/45 hover:bg-content/8 hover:text-content";
  return (
    <figure
      data-visual={document.id}
      className="group/visual my-3 flex min-w-0 flex-col gap-1.5 font-sans"
    >
      <div className="relative min-w-0 rounded-lg border border-content/10 p-4">
        <div className="absolute top-1.5 right-1.5 z-10 flex gap-0.5 opacity-0 transition-opacity group-hover/visual:opacity-100 focus-within:opacity-100">
          <button
            type="button"
            className={`${tool} ${source ? "bg-selection text-content" : ""}`}
            aria-pressed={source}
            aria-label={source ? "Show visual" : "Show source"}
            title={source ? "Show visual" : "Show source"}
            onClick={() => setSource((open) => !open)}
          >
            <Code size={14} />
          </button>
          <button
            type="button"
            className={tool}
            aria-label="Reset visual"
            title="Reset"
            onClick={() => {
              setSource(false);
              setGeneration((n) => n + 1);
            }}
          >
            <RotateCcw size={14} />
          </button>
        </div>
        {source ? (
          <div className="max-h-96 min-w-0 overflow-auto">
            <AgentMarkdown text={`\`\`\`js\n${document.source}\n\`\`\``} />
          </div>
        ) : error ? (
          <p className="text-[12px] text-content/60">
            This visual couldn’t run: {error}
          </p>
        ) : tree ? (
          <VisualTree node={tree} dispatch={dispatch} disabled={busy} />
        ) : (
          <div className="h-24" aria-busy="true" />
        )}
      </div>
      <figcaption className="px-1 text-[12px] text-content/50">
        {document.title}
      </figcaption>
    </figure>
  );
}
