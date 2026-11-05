import { useCallback, useEffect, useRef, useState } from "react";
import { subscribeMonos } from "../../monos/model/mono";
import { subscribeMonoFiles } from "../../monos/model/monoFiles";
import { BOT_EXPORT_FORMAT, BOT_EXPORT_VERSION } from "../model/botTransfer";
import {
  loadSyncFolder,
  runBotSync,
  settleBot,
  subscribeBotSync,
  subscribeBotSyncRequests,
  type SyncConflict,
} from "../model/botSyncIO";
import { BotImportDialog } from "./BotImportDialog";

/** How often the other computers' files are read again. */
const EVERY_MS = 2 * 60_000;
/** A change here is written out after this pause, so a burst writes once. */
const SETTLE_MS = 5_000;

const PLATFORMS: Record<string, string> = {
  Mac: "macos",
  Windows: "windows",
  Linux: "linux",
};

/**
 * Keeps the bots in step with the user's other computers through their sync
 * folder, while one is chosen. A bot changed on both opens the comparison.
 */
export function BotSyncRunner() {
  const [folder, setFolder] = useState(loadSyncFolder);
  const [conflicts, setConflicts] = useState<SyncConflict[]>([]);
  const running = useRef(false);
  const again = useRef(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => subscribeBotSync(() => setFolder(loadSyncFolder())), []);

  const sync = useCallback(async () => {
    if (running.current) {
      again.current = true;
      return;
    }
    running.current = true;
    try {
      const found = await runBotSync();
      // An open comparison is the user's: a later sync doesn't change it under them.
      if (found.length) setConflicts((open) => (open.length ? open : found));
    } finally {
      running.current = false;
      if (again.current) {
        again.current = false;
        void sync();
      }
    }
  }, []);

  useEffect(() => {
    if (!folder) return;
    const soon = () => {
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => void sync(), SETTLE_MS);
    };
    const first = window.setTimeout(() => void sync(), 1_500);
    const every = window.setInterval(() => void sync(), EVERY_MS);
    const stopRequests = subscribeBotSyncRequests(() => void sync());
    const stopMonos = subscribeMonos(soon);
    const stopFiles = subscribeMonoFiles(soon);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(every);
      window.clearTimeout(timer.current);
      stopRequests();
      stopMonos();
      stopFiles();
    };
  }, [folder, sync]);

  if (!conflicts.length) return null;
  return (
    <BotImportDialog
      file={{
        format: BOT_EXPORT_FORMAT,
        version: BOT_EXPORT_VERSION,
        exportedAt: "",
        platform: PLATFORMS[conflicts[0].machineName] ?? "",
        bots: conflicts.map((conflict) => conflict.bot),
      }}
      matches={conflicts.map((conflict) => conflict.localId)}
      onDecided={(choices) => {
        // Whatever was chosen, the other computer's version is now known here.
        for (const { bot } of choices) {
          const conflict = conflicts.find((entry) => entry.bot === bot);
          if (conflict) settleBot(conflict.bot.syncId, conflict.bot.hash);
        }
      }}
      onClose={() => setConflicts([])}
      onDone={() => {
        setConflicts([]);
        void sync();
      }}
    />
  );
}
