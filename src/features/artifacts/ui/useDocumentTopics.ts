import { useEffect, useRef, useSyncExternalStore } from "react";
import {
  canRunHarnessTextPrompt,
  pickTextHarness,
  runHarnessTextPrompt,
} from "../../../integrations/harness";
import { homeDir } from "../../../platform/tauri/fs";
import type { Block } from "../../sessions/model/session";
import { chatTopics, pendingTopicTurns } from "../documentGroups";
import {
  documentTopicsSnapshot,
  parseTopic,
  setDocumentTopic,
  subscribeDocumentTopics,
  topicPrompt,
} from "../documentTopics";

const TOPIC_TIMEOUT_MS = 30_000;

/**
 * Names the topic of each request that left documents, once the bot is done,
 * one request at a time and oldest first, so a later request can join a
 * topic named before it. The cheapest model the text harness has does it:
 * Claude's Haiku, at low effort. Without one, folders keep their requests'
 * first lines. Returns every turn's topic.
 */
export function useDocumentTopics(
  blocks: readonly Block[] | undefined,
  busy: boolean,
  enabled: boolean,
): Readonly<Record<string, string>> {
  const topics = useSyncExternalStore(
    subscribeDocumentTopics,
    documentTopicsSnapshot,
  );
  const running = useRef(false);
  // A request the model could not name is not asked again in this window.
  const failed = useRef(new Set<string>());

  useEffect(() => {
    if (!enabled || busy || !blocks || running.current) return;
    const next = pendingTopicTurns(blocks, topics).find(
      (turn) => !failed.current.has(turn.turnId),
    );
    if (!next) return;
    const harness = pickTextHarness();
    if (!canRunHarnessTextPrompt(harness)) return;
    const existing = chatTopics(blocks, topics);
    running.current = true;
    void (async () => {
      try {
        const output = await runHarnessTextPrompt({
          harness,
          cwd: await homeDir(),
          prompt: topicPrompt(next, existing),
          timeoutMs: TOPIC_TIMEOUT_MS,
          ...(harness === "claude" ? { modelSettings: { effort: "low" } } : {}),
        });
        const topic = parseTopic(output, existing);
        if (topic) setDocumentTopic(next.turnId, topic);
        else failed.current.add(next.turnId);
      } catch {
        failed.current.add(next.turnId);
      } finally {
        running.current = false;
      }
    })();
  }, [blocks, busy, enabled, topics]);

  return topics;
}
