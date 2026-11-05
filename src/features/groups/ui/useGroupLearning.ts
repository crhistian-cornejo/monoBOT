import { useEffect, useRef } from "react";
import {
  canRunHarnessTextPrompt,
  pickTextHarness,
  runHarnessTextPrompt,
} from "../../../integrations/harness";
import { homeDir } from "../../../platform/tauri/fs";
import { listMonos, monoLook } from "../../monos/model/mono";
import { readAgentFile, writeAgentFile } from "../../monos/model/monoFiles";
import type { Session } from "../../sessions/model/session";
import { groupLead } from "../model/groupBrief";
import {
  learnedUpTo,
  learningPrompt,
  markLearned,
  parseLearned,
  pendingExchange,
} from "../model/groupLearning";
import { memorySeed } from "../model/groupProject";

const LEARN_TIMEOUT_MS = 45_000;

/**
 * Saves what each group's chat settled into the project's memory, one
 * group at a time, once its chat is idle. The cheapest model the text
 * harness has does it; without one, the group keeps what its lead saved.
 */
export function useGroupLearning(sessions: readonly Session[]) {
  const running = useRef(false);
  // An exchange the model could not read is not tried again this window.
  const failed = useRef(new Set<string>());

  useEffect(() => {
    if (running.current) return;
    const roster = listMonos();
    for (const mono of roster) {
      if (!mono.group || !mono.sessionId) continue;
      const chat = sessions.find((entry) => entry.id === mono.sessionId);
      if (!chat || chat.busy) continue;
      // Wait for the members still answering: their part is the exchange.
      const seats = mono.group.seats ?? {};
      const memberBusy = mono.group.members.some((id) =>
        sessions.some((entry) => entry.id === seats[id] && entry.busy),
      );
      if (memberBusy) continue;
      const lead = groupLead(mono.group);
      const exchange = pendingExchange(
        chat.blocks,
        learnedUpTo(chat.id),
        lead ? monoLook(lead).name : "Lead",
      );
      if (!exchange || failed.current.has(exchange.lastId)) continue;
      const harness = pickTextHarness();
      if (!canRunHarnessTextPrompt(harness)) return;
      running.current = true;
      const monoId = mono.id;
      const name = monoLook(mono).name;
      const goal = mono.group.goal;
      const chatId = chat.id;
      void (async () => {
        try {
          const current = await readAgentFile(monoId, "MEMORY.md");
          const memory = current.text ?? "";
          const output = await runHarnessTextPrompt({
            harness,
            cwd: await homeDir(),
            prompt: learningPrompt(name, goal, memory, exchange),
            timeoutMs: LEARN_TIMEOUT_MS,
            ...(harness === "claude"
              ? { modelSettings: { effort: "low" } }
              : {}),
          });
          const facts = parseLearned(output, memory);
          if (facts.length) {
            const date = new Date().toISOString().slice(0, 10);
            const next = memorySeed(memory, facts.join("\n"), date);
            if (next !== memory)
              await writeAgentFile(monoId, "MEMORY.md", next, current.hash);
          }
          markLearned(chatId, exchange.lastId);
        } catch (error) {
          failed.current.add(exchange.lastId);
          console.warn("Could not save what the group learned", error);
        } finally {
          running.current = false;
        }
      })();
      return;
    }
  }, [sessions]);
}
