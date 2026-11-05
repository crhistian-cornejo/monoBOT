import type { ProviderRetry, ProviderRetryReason, Session } from "./session";
import { usageLimitFromError } from "./usageLimit";

/** Automatic retries after the failed turn, before giving up. */
export const PROVIDER_RETRY_MAX = 4;

/** The first wait; each later retry waits twice as long as the one before. */
const FIRST_WAIT_MS: Record<ProviderRetryReason, number> = {
  overloaded: 60_000,
  server: 60_000,
  connection: 15_000,
};

/**
 * Up to this fraction is added to each wait, so sessions of one account that
 * failed together don't all retry in the same second.
 */
const JITTER = 0.25;

const OVERLOADED =
  /overloaded|\b529\b|high demand|at capacity|capacity constraints|too many requests|\b429\b|rate[_ ]limit(?:ed|_error)?\b/i;
const SERVER =
  /\b(?:api error|http|status(?: code)?|error)\W{0,3}5\d\d\b|internal server error|bad gateway|service unavailable|gateway time-?out|\bserver_error\b|\bapi_error\b/i;
const CONNECTION =
  /ECONNRESET|ECONNREFUSED|ETIMEDOUT|ENOTFOUND|EAI_AGAIN|EPIPE|socket hang up|network error|fetch failed|connection (?:error|reset|refused|closed|lost)|stream disconnected|error sending request|request timed out/i;

/**
 * A transient failure worth retrying. Usage, credit and spending limits are
 * not: waiting a few minutes won't lift them, and they have their own notice.
 */
export function providerRetryReason(
  message: string,
): ProviderRetryReason | undefined {
  if (usageLimitFromError(message)) return undefined;
  if (OVERLOADED.test(message)) return "overloaded";
  if (SERVER.test(message)) return "server";
  if (CONNECTION.test(message)) return "connection";
  return undefined;
}

/** How long to wait before retry number `retry` (1-based); `random` in [0, 1). */
export function providerRetryWait(
  reason: ProviderRetryReason,
  retry: number,
  random: number,
): number {
  const wait = FIRST_WAIT_MS[reason] * 2 ** (retry - 1);
  return Math.round(wait * (1 + JITTER * random));
}

/** The retry state after the provider failed the turn with `message`. */
export function nextProviderRetry(
  session: Session,
  message: string,
  now: number,
  random = Math.random(),
): ProviderRetry | undefined {
  const reason = providerRetryReason(message);
  if (!reason || session.usageLimit) return undefined;
  const previous = session.providerRetry;
  // The same failed turn reporting a second error: it isn't another attempt.
  if (previous && !previous.sending) return previous;
  const attempt = (previous?.attempt ?? 0) + 1;
  // Scheduled runs come back on their own schedule; don't add turns to them.
  const automatic =
    attempt <= PROVIDER_RETRY_MAX && !session.automationId && !session.ephemeral;
  return {
    reason,
    attempt,
    ...(automatic
      ? { retryAt: now + providerRetryWait(reason, attempt, random) }
      : {}),
  };
}

/** Idle and past the wait: time to send the retry. */
export function providerRetryDue(session: Session, now: number): boolean {
  const retry = session.providerRetry;
  return (
    retry?.retryAt != null && !retry.sending && !session.busy && now >= retry.retryAt
  );
}

/** What the retry turn carries, so a further failure counts as the next attempt. */
export function sendingProviderRetry(retry: ProviderRetry): ProviderRetry {
  return { reason: retry.reason, attempt: retry.attempt, sending: true };
}

/** The retried turn ended without an error. */
export function providerRetrySucceeded(session: Session): boolean {
  return !!session.providerRetry?.sending && !session.busy;
}

const REASON_LABEL: Record<ProviderRetryReason, string> = {
  overloaded: "Provider is overloaded",
  server: "Provider error",
  connection: "Connection lost",
};

export function providerRetryTitle(retry: ProviderRetry): string {
  return REASON_LABEL[retry.reason];
}

/** "Retry 2 of 4 in 1m 30s", or why it won't retry by itself. */
export function providerRetryStatus(retry: ProviderRetry, now: number): string {
  if (retry.retryAt == null) {
    return retry.attempt > PROVIDER_RETRY_MAX
      ? `Gave up after ${PROVIDER_RETRY_MAX} retries`
      : "Won't retry automatically";
  }
  const left = retry.retryAt - now;
  const counter = `Retry ${retry.attempt} of ${PROVIDER_RETRY_MAX}`;
  return left > 0 ? `${counter} in ${formatWait(left)}` : `${counter} now`;
}

function formatWait(ms: number): string {
  const seconds = Math.ceil(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest ? `${minutes}m ${rest}s` : `${minutes}m`;
}
