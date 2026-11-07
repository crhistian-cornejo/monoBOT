import type { HarnessId } from "../../../features/sessions/model/session";
import type {
  ProviderRateLimits,
  RateLimitProvider,
} from "../../../features/providers/model/rateLimits";
import {
  getAllRateLimits,
  getCachedRateLimits,
} from "../../../features/providers/model/rateLimitsCache";
import { isHarnessAvailable } from "./availability";
import type { PrContent } from "../../../features/source-control/model/gitText";
import {
  generateHarnessCommitMessage,
  generateHarnessPrContent,
  warmupHarnessText,
} from "./registry";

export const ROUTER_MIN_HEADROOM = 40;

const TEXT_HARNESSES: HarnessId[] = [
  "claude",
  "cursor",
  "codex",
  "devin",
  "grok",
  "opencode",
];

const RATE_LIMIT_HARNESSES = new Set<RateLimitProvider>([
  "claude",
  "codex",
  "devin",
  "opencode",
]);

function isRateLimitProvider(id: HarnessId): id is RateLimitProvider {
  return RATE_LIMIT_HARNESSES.has(id as RateLimitProvider);
}

/** Remaining headroom (0–100) across rate-limit windows, resilient to clock skew. */
export function routerHeadroom(
  limits?: ProviderRateLimits,
  now = Date.now(),
): number | null {
  if (!limits) return null;
  if (limits.windows.length === 0) {
    return limits.error ? 0 : null;
  }
  let min = 100;
  for (const w of limits.windows) {
    const reset =
      w.resetsAt != null &&
      (limits.updatedAt <= 0 || w.resetsAt > limits.updatedAt) &&
      w.resetsAt <= now;
    const clamped = Math.min(100, Math.max(0, w.usedPercent));
    const left = reset ? 100 : Math.max(0, 100 - clamped);
    if (left < min) min = left;
  }
  return min;
}

function harnessHeadroom(id: HarnessId, now = Date.now()): number | null {
  if (!isRateLimitProvider(id)) return null;
  const all = getAllRateLimits();
  const prefix = `${id}:`;
  let best: number | null = null;
  for (const [key, snapshot] of Object.entries(all)) {
    if (!key.startsWith(prefix)) continue;
    const room = routerHeadroom(snapshot, now);
    if (room != null && (best == null || room > best)) {
      best = room;
    }
  }
  if (best != null) return best;
  return routerHeadroom(getCachedRateLimits(id), now);
}

/**
 * Orders available text harnesses so that when Claude has no usage (or <= 40%
 * remaining headroom), providers with > 40% usage headroom are routed first.
 */
export function routeTextHarnesses(preferred?: HarnessId): HarnessId[] {
  const ordered =
    preferred && TEXT_HARNESSES.includes(preferred)
      ? [preferred, ...TEXT_HARNESSES.filter((id) => id !== preferred)]
      : TEXT_HARNESSES;
  const available = ordered.filter((id) => isHarnessAvailable(id));
  if (available.length === 0) {
    return [
      preferred && TEXT_HARNESSES.includes(preferred) ? preferred : "cursor",
    ];
  }
  const now = Date.now();
  const rooms = new Map<HarnessId, number | null>(
    available.map((id) => [id, harnessHeadroom(id, now)]),
  );
  const first = available[0];
  const firstRoom = rooms.get(first) ?? null;
  const hasAboveThreshold = available.some(
    (id) => (rooms.get(id) ?? 0) > ROUTER_MIN_HEADROOM,
  );
  const rotateAway =
    (firstRoom != null && firstRoom <= ROUTER_MIN_HEADROOM) ||
    (first === "claude" &&
      !(firstRoom != null && firstRoom > ROUTER_MIN_HEADROOM) &&
      hasAboveThreshold);

  const sortCandidates = (list: HarnessId[]): HarnessId[] => {
    const above: Array<{ id: HarnessId; room: number }> = [];
    const unknown: HarnessId[] = [];
    const low: Array<{ id: HarnessId; room: number }> = [];
    const exhausted: HarnessId[] = [];
    for (const id of list) {
      const room = rooms.get(id) ?? null;
      if (room != null && room > ROUTER_MIN_HEADROOM) {
        above.push({ id, room });
      } else if (room == null) {
        unknown.push(id);
      } else if (room > 0) {
        low.push({ id, room });
      } else {
        exhausted.push(id);
      }
    }
    above.sort((a, b) => b.room - a.room);
    low.sort((a, b) => b.room - a.room);
    return [
      ...above.map((entry) => entry.id),
      ...unknown,
      ...low.map((entry) => entry.id),
      ...exhausted,
    ];
  };

  if (rotateAway) {
    return sortCandidates(available);
  }
  return [first, ...sortCandidates(available.slice(1))];
}

/** Pick the harness used for titles, commit messages, and PR text. */
export function pickTextHarness(preferred?: HarnessId): HarnessId {
  return routeTextHarnesses(preferred)[0];
}

export function warmupText(cwd: string, preferred?: HarnessId): Promise<void> {
  return warmupHarnessText(pickTextHarness(preferred), cwd);
}

export function generateCommitMessage(
  cwd: string,
  preferred?: HarnessId,
  signal?: AbortSignal,
  paths?: readonly string[],
): Promise<string> {
  return generateHarnessCommitMessage(pickTextHarness(preferred), cwd, signal, paths);
}

export function generatePrContent(
  cwd: string,
  preferred?: HarnessId,
): Promise<(PrContent & { base: string; head: string }) | null> {
  return generateHarnessPrContent(pickTextHarness(preferred), cwd);
}
