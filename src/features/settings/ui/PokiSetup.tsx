import { useCallback, useEffect, useState } from "react";
import {
  homeDir,
  pickFolders,
  readTextFile,
  statFiles,
} from "../../../platform/tauri/fs";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { Folder, RefreshCw, Shield } from "../../../shared/ui/icons";
import { connectorCheckout, setConnectorCheckout } from "../model/connectors";
import {
  expandHome,
  pokiFolderProblem,
  pokiStatus,
  POKI_STATUS_TEXT,
  type PokiStatus,
} from "../model/pokiSetup";
import { PokiTutorial } from "./PokiTutorial";

const io = {
  exists: async (path: string) => (await statFiles([path]))[0]?.mtimeMs != null,
  read: readTextFile,
};

const TONE: Record<PokiStatus, string> = {
  "no-folder": "bg-content/30",
  "no-bridge": "bg-diff-del",
  stopped: "bg-diff-del",
  ready: "bg-diff-add",
};

/** Poki's folder, whether its daemon runs, the tutorial and the rules. */
export function PokiSetup() {
  const [folder, setFolder] = useState(() => connectorCheckout("poki"));
  const [draft, setDraft] = useState(folder);
  const [status, setStatus] = useState<PokiStatus | null>(null);
  const [tutorial, setTutorial] = useState(false);
  const problem = pokiFolderProblem(draft);

  const check = useCallback(async (path: string) => {
    setStatus(null);
    setStatus(await pokiStatus(path, io).catch(() => "stopped" as const));
  }, []);

  useEffect(() => {
    void check(folder);
  }, [check, folder]);

  const save = async (path: string) => {
    if (pokiFolderProblem(path)) return;
    const full = expandHome(path, await homeDir());
    setConnectorCheckout("poki", full);
    setDraft(full);
    setFolder(full);
  };

  const text = status ? POKI_STATUS_TEXT[status] : null;
  return (
    <div className="space-y-3 border-t border-stroke bg-content/[0.02] px-3 py-3">
      <form
        className="space-y-1.5"
        onSubmit={(event) => {
          event.preventDefault();
          void save(draft);
        }}
      >
        <label
          htmlFor="poki-folder"
          className="block text-[12px] text-content/70"
        >
          Poki folder
        </label>
        <div className="flex items-center gap-2">
          <input
            id="poki-folder"
            value={draft}
            spellCheck={false}
            placeholder="~/poki"
            onChange={(event) => setDraft(event.target.value)}
            onBlur={() => {
              if (draft.trim() !== folder) void save(draft);
            }}
            className="h-9 min-w-0 flex-1 rounded-md border border-content/10 bg-background-base px-2.5 font-mono text-[13px] outline-none focus:border-content/25"
          />
          <SecondaryButton
            onClick={async () => {
              const [picked] = await pickFolders("Choose the Poki folder");
              if (picked) void save(picked);
            }}
          >
            <Folder className="size-3.5" />
            Choose…
          </SecondaryButton>
        </div>
        {problem ? <p className="text-xs text-diff-del">{problem}</p> : null}
      </form>

      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2" aria-live="polite">
          <span
            className={`size-2 shrink-0 rounded-full ${status ? TONE[status] : "animate-pulse bg-content/30"}`}
          />
          <div className="min-w-0">
            <p className="text-[12px] font-medium">
              {text?.label ?? "Checking…"}
            </p>
            {text ? (
              <p className="text-xs text-content/55">{text.hint}</p>
            ) : null}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <SecondaryButton onClick={() => void check(folder)}>
            <RefreshCw className="size-3.5" />
            Check again
          </SecondaryButton>
          <button
            type="button"
            onClick={() => setTutorial(true)}
            className="primary-action rounded-md px-3 py-1.5 text-[12px]"
          >
            How to set it up
          </button>
        </div>
      </div>

      <p className="flex gap-2 text-xs text-content/55">
        <Shield className="mt-px size-3.5 shrink-0" />
        <span>
          It acts on your real phone. Only a bot you give it to can use it;
          every tap, swipe and keystroke asks you first. It never reads or
          enters codes, never unlocks the phone and never accepts a purchase.
          Screens are sent to the model you use. Start Poki without{" "}
          <code className="font-mono">POKI_MCP_APPROVAL_MODE=allow</code>.
        </span>
      </p>

      {tutorial ? (
        <PokiTutorial
          status={status}
          onClose={() => {
            setTutorial(false);
            void check(connectorCheckout("poki"));
          }}
        />
      ) : null}
    </div>
  );
}
