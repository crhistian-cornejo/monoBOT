import { TextQuote, X } from "../../../shared/ui/icons";
import {
  quoteChipLabel,
  quoteChipSource,
  splitQuoteSource,
} from "../model/composerQuotes";

/**
 * Text added to the chat, waiting in the composer: the start of the quote and,
 * for one taken from a paper, where it is from. The whole quote shows on hover
 * and goes out with the message.
 */
export function QuoteChip({
  text,
  onRemove,
}: {
  text: string;
  onRemove?: () => void;
}) {
  const label = quoteChipLabel(text);
  const source = quoteChipSource(text);
  return (
    <div
      className="group relative flex min-w-0 max-w-[280px] items-center gap-1.5 rounded-md bg-content/10 py-1 pl-1.5 pr-1"
      title={splitQuoteSource(text).body}
    >
      <TextQuote
        aria-hidden="true"
        className="size-3.5 shrink-0 text-content/55"
        strokeWidth={1.75}
      />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-[11px] leading-none text-content/80">
          {label}
        </span>
        {source ? (
          <span className="truncate text-[10px] leading-none text-content/50">
            {source}
          </span>
        ) : null}
      </span>
      {onRemove ? (
        <button
          type="button"
          title="Remove"
          aria-label="Remove quote"
          onClick={(event) => {
            event.stopPropagation();
            onRemove();
          }}
          className="grid size-4 shrink-0 place-items-center rounded-full text-content/40 hover:bg-content/15 hover:text-content"
        >
          <X className="size-3" strokeWidth={2} />
        </button>
      ) : null}
    </div>
  );
}
