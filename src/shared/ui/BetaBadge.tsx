/** Marks a feature that still changes from one version to the next. */
export function BetaBadge({
  className = "",
  label = "Beta",
}: {
  className?: string;
  label?: string;
}) {
  return (
    <span
      className={`inline-flex h-4 shrink-0 items-center rounded bg-accent/10 px-1 text-[10px] font-medium uppercase leading-none tracking-wide text-accent ${className}`}
    >
      {label}
    </span>
  );
}
