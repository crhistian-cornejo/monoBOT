import { findMono, monoLook } from "../../monos/model/mono";
import { readAgentFile, writeAgentFile } from "../../monos/model/monoFiles";
import type { BotFolder } from "../../bots/model/bots";
import { addMemoryEntry, memoryEntry } from "../../monos/model/monoMemory";

/** A group as its settings dialog edits it. */
export function groupDraft(monoId: string):
  | {
      name: string;
      goal: string;
      members: string[];
      folders: BotFolder[];
      context: string[];
    }
  | undefined {
  const mono = findMono(monoId);
  if (!mono?.group) return undefined;
  return {
    name: monoLook(mono).name,
    goal: mono.group.goal ?? "",
    members: [...mono.group.members],
    folders: [...(mono.group.folders ?? [])],
    context: [...(mono.group.context ?? [])],
  };
}

/**
 * What the user wrote for a new group to remember, as memory entries: one
 * dated entry per line, the way memory.add writes them.
 */
export function memorySeed(memory: string, text: string, date: string): string {
  return text
    .split("\n")
    .map((line) => line.replace(/^\s*[-*]\s*/, "").trim())
    .filter(Boolean)
    .reduce(
      (current, fact) => addMemoryEntry(current, memoryEntry(fact, date)).text,
      memory,
    );
}

/** Writes a new group's first memories; a failure leaves it empty. */
export async function seedGroupMemory(
  monoId: string,
  text: string,
): Promise<void> {
  try {
    const current = await readAgentFile(monoId, "MEMORY.md");
    const before = current.text ?? "";
    const next = memorySeed(
      before,
      text,
      new Date().toISOString().slice(0, 10),
    );
    if (next !== before)
      await writeAgentFile(monoId, "MEMORY.md", next, current.hash);
  } catch {
    // The group starts with an empty memory; it learns as it goes.
  }
}
