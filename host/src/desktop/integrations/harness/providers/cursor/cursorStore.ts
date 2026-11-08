// Cursor's native session database lives with the desktop app. The headless
// host has no copy, so stored enrichment is always empty here; the adapter
// keeps the transcript and subagent events Cursor streams.

export type StoredCursorToolCall = {
  toolCallId: string;
  toolName: string;
  args: unknown;
};

export type StoredCursorSubagentRun = {
  toolCallId: string;
  agentId: string;
  revision: string;
  agentType?: string | null;
  model?: string | null;
  prompt?: string | null;
  steps: Array<{
    id: string;
    kind: "message" | "reasoning" | "tool";
    text: string;
    toolName?: string;
    args?: unknown;
    status?: string;
    output?: string;
  }>;
};

export function readStoredCursorSubagentRuns(
  _sessionId: string,
  _toolCallIds: string[],
  _knownRevisions: Record<string, string> = {},
): Promise<StoredCursorSubagentRun[]> {
  return Promise.resolve([]);
}

export function readStoredCursorToolCalls(
  _sessionId: string,
  _toolCallIds: string[],
): Promise<StoredCursorToolCall[]> {
  return Promise.resolve([]);
}
