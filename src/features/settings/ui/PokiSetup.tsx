import { ask } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  homeDir,
  pickFolders,
  readTextFile,
  statFiles,
} from "../../../platform/tauri/fs";
import {
  installPoki,
  onPokiInstall,
  pokiMachine,
  startPoki,
} from "../../../platform/tauri/poki";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { Folder, Play, RefreshCw, Shield } from "../../../shared/ui/icons";
import { connectorCheckout, setConnectorCheckout } from "../model/connectors";
import {
  expandHome,
  pokiFolderProblem,
  pokiInstallQuestion,
  pokiNext,
  pokiStatus,
  POKI_STAGE_TEXT,
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

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

/**
 * Poki's folder, whether its daemon runs, the tutorial and the rules.
 * `activate` is set when the user has just turned the switch on: then it
 * installs Poki (after asking) when it is missing and starts its daemon.
 */
export function PokiSetup({ activate = false }: { activate?: boolean }) {
  const [folder, setFolder] = useState(() => connectorCheckout("poki"));
  const [draft, setDraft] = useState(folder);
  const [status, setStatus] = useState<PokiStatus | null>(null);
  const [tutorial, setTutorial] = useState(false);
  /** What it is doing now, when it is doing something. */
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const activated = useRef(false);
  const problem = pokiFolderProblem(draft);

  const check = useCallback(async (path: string) => {
    setStatus(null);
    const next = await pokiStatus(path, io).catch(() => "stopped" as const);
    setStatus(next);
    return next;
  }, []);

  const save = async (path: string) => {
    if (pokiFolderProblem(path)) return "";
    const full = expandHome(path, await homeDir());
    setConnectorCheckout("poki", full);
    setDraft(full);
    setFolder(full);
    return full;
  };

  const start = useCallback(
    async (path: string) => {
      setBusy("Starting Poki…");
      try {
        await startPoki(path);
      } catch (cause) {
        setError(message(cause));
      } finally {
        setBusy(null);
        await check(path);
      }
    },
    [check],
  );

  /** Install when missing, then start: what the switch promises. */
  const bringUp = useCallback(async () => {
    setError(null);
    const saved = connectorCheckout("poki");
    const [current, machine] = await Promise.all([
      check(saved),
      pokiMachine().catch(() => null),
    ]);
    if (!machine) return;
    const next = pokiNext(saved, current, machine);
    switch (next.kind) {
      case "ready":
        return;
      case "unsupported":
        setError(
          "Poki's package runs on Apple Silicon Macs only. The tutorial shows how to build it yourself.",
        );
        return;
      case "missing":
        setError(
          `Install first, then turn it on again: ${next.missing.join("; ")}.`,
        );
        return;
      case "start":
        await start(next.folder);
        return;
      case "use":
        await start(await save(next.folder));
        return;
      case "install": {
        const yes = await ask(pokiInstallQuestion(machine), {
          title: "Install Poki",
          kind: "info",
          okLabel: "Download and install",
          cancelLabel: "Not now",
        });
        if (!yes) return;
        const unlisten = await onPokiInstall((stage) =>
          setBusy(POKI_STAGE_TEXT[stage]),
        );
        setBusy(POKI_STAGE_TEXT.download);
        try {
          const installed = await installPoki();
          await start(await save(installed));
        } catch (cause) {
          setError(message(cause));
          setBusy(null);
        } finally {
          unlisten();
        }
      }
    }
  }, [check, start]);

  useEffect(() => {
    if (activate && !activated.current) {
      activated.current = true;
      void bringUp();
    } else {
      void check(folder);
    }
  }, [activate, bringUp, check, folder]);

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
            disabled={busy !== null}
            placeholder="Installed by MonoBOT, or ~/poki"
            onChange={(event) => setDraft(event.target.value)}
            onBlur={() => {
              if (draft.trim() !== folder) void save(draft);
            }}
            className="h-9 min-w-0 flex-1 rounded-md border border-content/10 bg-background-base px-2.5 font-mono text-[13px] outline-none focus:border-content/25 disabled:opacity-60"
          />
          <SecondaryButton
            disabled={busy !== null}
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
            className={`size-2 shrink-0 rounded-full ${
              busy || !status ? "animate-pulse bg-accent" : TONE[status]
            }`}
          />
          <div className="min-w-0">
            <p className="text-[12px] font-medium">
              {busy ?? text?.label ?? "Checking…"}
            </p>
            {!busy && text ? (
              <p className="text-xs text-content/55">{text.hint}</p>
            ) : null}
          </div>
        </div>
        <div className="flex items-center gap-2">
          {status === "stopped" && !busy ? (
            <SecondaryButton onClick={() => void start(folder)}>
              <Play className="size-3.5" />
              Start Poki
            </SecondaryButton>
          ) : status === "no-folder" && !busy ? (
            <SecondaryButton onClick={() => void bringUp()}>
              Install Poki
            </SecondaryButton>
          ) : (
            <SecondaryButton
              disabled={busy !== null}
              onClick={() => void check(folder)}
            >
              <RefreshCw className="size-3.5" />
              Check again
            </SecondaryButton>
          )}
          <button
            type="button"
            onClick={() => setTutorial(true)}
            className="primary-action rounded-md px-3 py-1.5 text-[12px]"
          >
            How to set it up
          </button>
        </div>
      </div>

      {error ? (
        <p className="whitespace-pre-wrap rounded-md border border-content/10 px-2.5 py-2 text-xs text-diff-del">
          {error}
        </p>
      ) : null}

      <p className="flex gap-2 text-xs text-content/55">
        <Shield className="mt-px size-3.5 shrink-0" />
        <span>
          It acts on your real phone. Only a bot you give it to can use it;
          every tap, swipe and keystroke asks you first. It never reads or
          enters codes, never unlocks the phone and never accepts a purchase.
          Screens are sent to the model you use. MonoBOT installs only the
          published Poki release, checked against its SHA-256, and never starts
          it with{" "}
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
