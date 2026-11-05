import { useState } from "react";
import {
  DateTimePicker,
  parseLocalDateTime,
  toLocalDateTime,
} from "./DateTimePicker";

type Props = {
  /** Shown above the calendar, e.g. "Mute all notifications until". */
  title: string;
  submitLabel: string;
  /** Epoch ms preselected; rounded up to the minute. */
  initial: number;
  disabled?: boolean;
  /** Return an error to show, or nothing once the date is taken. */
  onSubmit: (at: number) => string | void;
  onCancel: () => void;
};

/** Pick a future date and time: a calendar, a time, Cancel and a primary action. */
export function DateTimeForm({
  title,
  submitLabel,
  initial,
  disabled,
  onSubmit,
  onCancel,
}: Props) {
  const [value, setValue] = useState(() =>
    toLocalDateTime(new Date(Math.ceil(initial / 60_000) * 60_000)),
  );
  const [error, setError] = useState<string | null>(null);
  return (
    <form
      noValidate
      className="w-full"
      onSubmit={(event) => {
        event.preventDefault();
        if (disabled) return;
        const date = parseLocalDateTime(value);
        if (!date) {
          setError("Choose a valid date and time.");
          return;
        }
        if (date.getTime() <= Date.now()) {
          setError("Choose a date and time in the future.");
          return;
        }
        setError(onSubmit(date.getTime()) || null);
      }}
    >
      <p className="mb-3 px-1 text-[11px] text-content/45">{title}</p>
      <DateTimePicker
        value={value}
        onChange={(next) => {
          setValue(next);
          setError(null);
        }}
        minDate={toLocalDateTime(new Date(Date.now())).slice(0, 10)}
        autoFocus
      />
      {error ? (
        <p role="alert" className="mt-3 px-1 text-xs text-red-400">
          {error}
        </p>
      ) : null}
      <div className="mt-3 flex items-center justify-between gap-2 border-t border-stroke pt-2.5">
        <button
          type="button"
          onClick={onCancel}
          className="rounded px-2 py-1.5 text-xs text-content/50 hover:bg-content/5 hover:text-content focus-visible:outline-2 focus-visible:outline-accent"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={disabled}
          className="primary-action flex shrink-0 items-center rounded-md border border-transparent px-2.5 py-1 text-[12px] focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-default"
        >
          {submitLabel}
        </button>
      </div>
    </form>
  );
}
