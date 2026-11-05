import { useState } from "react";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import type { BotExport } from "../model/botTransfer";
import { exportBotsToFile, pickBotExport } from "../model/botTransferIO";
import { BotImportDialog } from "./BotImportDialog";

/** Export every bot to a file, or import one from another computer. */
export function BotTransferControls() {
  const [file, setFile] = useState<BotExport | null>(null);
  const [status, setStatus] = useState<{ text: string; error?: boolean }>();

  const run = async (task: () => Promise<void>) => {
    setStatus(undefined);
    try {
      await task();
    } catch (failure) {
      setStatus({
        text: failure instanceof Error ? failure.message : String(failure),
        error: true,
      });
    }
  };

  return (
    <div className="flex flex-col items-end gap-1.5">
      <div className="flex gap-2">
        <SecondaryButton
          onClick={() =>
            void run(async () => {
              const done = await exportBotsToFile();
              if (done)
                setStatus({
                  text: `${done.count} bot${done.count === 1 ? "" : "s"} saved to ${done.path}`,
                });
            })
          }
        >
          Export bots
        </SecondaryButton>
        <SecondaryButton
          onClick={() =>
            void run(async () => {
              const picked = await pickBotExport();
              if (picked?.bots.length) setFile(picked);
              else if (picked) setStatus({ text: "That file has no bots in it." });
            })
          }
        >
          Import bots
        </SecondaryButton>
      </div>
      {status ? (
        <p
          role={status.error ? "alert" : "status"}
          className={`max-w-80 break-words text-right text-[11px] ${status.error ? "text-red-400" : "text-content/50"}`}
        >
          {status.text}
        </p>
      ) : null}
      {file ? (
        <BotImportDialog
          file={file}
          onClose={() => setFile(null)}
          onDone={(summary) => {
            setFile(null);
            setStatus({ text: summary });
          }}
        />
      ) : null}
    </div>
  );
}
