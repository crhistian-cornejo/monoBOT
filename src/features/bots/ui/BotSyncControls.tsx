import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import {
  loadSyncFolder,
  loadSyncStatus,
  requestBotSync,
  saveSyncFolder,
  subscribeBotSync,
} from "../model/botSyncIO";

/** Sync the bots through a folder the user already syncs between computers. */
export function BotSyncControls() {
  const [folder, setFolder] = useState(loadSyncFolder);
  const [status, setStatus] = useState(loadSyncStatus);

  useEffect(
    () =>
      subscribeBotSync(() => {
        setFolder(loadSyncFolder());
        setStatus(loadSyncStatus());
      }),
    [],
  );

  const choose = async () => {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "Folder your computers share (OneDrive, iCloud Drive, Dropbox…)",
    });
    if (typeof picked !== "string") return;
    saveSyncFolder(picked);
    requestBotSync();
  };

  return (
    <div className="flex flex-col items-end gap-1.5">
      <div className="flex gap-2">
        {folder ? (
          <>
            <SecondaryButton onClick={requestBotSync}>Sync now</SecondaryButton>
            <SecondaryButton onClick={() => saveSyncFolder(undefined)}>Turn off</SecondaryButton>
          </>
        ) : null}
        <SecondaryButton onClick={() => void choose()}>
          {folder ? "Change folder" : "Choose folder"}
        </SecondaryButton>
      </div>
      {folder ? (
        <p className="max-w-80 break-all text-right text-[11px] text-content/50" title={folder}>
          {folder}
        </p>
      ) : null}
      {folder && status ? (
        <p
          role={status.error ? "alert" : "status"}
          className={`max-w-80 text-right text-[11px] ${status.error ? "text-red-400" : "text-content/50"}`}
        >
          {status.text} ·{" "}
          {new Date(status.at).toLocaleTimeString(undefined, {
            hour: "numeric",
            minute: "2-digit",
          })}
        </p>
      ) : null}
    </div>
  );
}
