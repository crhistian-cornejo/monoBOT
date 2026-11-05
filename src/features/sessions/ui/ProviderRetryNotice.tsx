import { useEffect, useState } from "react";
import { RefreshCw, RotateCcw, X } from "../../../shared/ui/icons";
import type { ProviderRetry } from "../model/session";
import {
  providerRetryStatus,
  providerRetryTitle,
} from "../model/providerRetry";
import {
  COMPOSER_NOTICE_BAR,
  COMPOSER_NOTICE_BUTTON,
} from "./UsageLimitNotice";

/**
 * A transient provider failure, and when the turn is sent again. Hidden while
 * the retry runs: that turn shows its own progress.
 */
export function ProviderRetryNotice({
  retry,
  onRetryNow,
  onCancel,
}: {
  retry: ProviderRetry;
  onRetryNow?: () => void;
  onCancel?: () => void;
}) {
  const [now, setNow] = useState(Date.now);
  const counting = retry.retryAt != null;
  // The waits are seconds to minutes long, so tick every second.
  useEffect(() => {
    if (!counting) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [counting]);

  return (
    <div className="px-2 text-content/55" data-provider-retry role="status">
      <div className={COMPOSER_NOTICE_BAR}>
        <RefreshCw className="size-3.5 shrink-0 text-amber-400" />
        <span className="shrink-0 text-content/85">
          {providerRetryTitle(retry)}
        </span>
        <span className="min-w-0 flex-1 truncate">
          {providerRetryStatus(retry, now)}
        </span>
        <button
          type="button"
          onClick={onRetryNow}
          className={COMPOSER_NOTICE_BUTTON}
        >
          <RotateCcw className="size-3.5" />
          Retry now
        </button>
        {onCancel ? (
          <button
            type="button"
            title={retry.retryAt != null ? "Don't retry" : "Dismiss"}
            aria-label="Cancel the automatic retry"
            onClick={onCancel}
            className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content"
          >
            <X className="size-3.5" />
          </button>
        ) : null}
      </div>
    </div>
  );
}
