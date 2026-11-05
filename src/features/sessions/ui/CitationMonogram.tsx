/** The first letter of a site's host, standing in for its icon. */
export function CitationMonogram({
  host,
  inverted = false,
  className = "size-4",
}: {
  host: string;
  /** On the filled (active) chip, where the ink and the ground swap. */
  inverted?: boolean;
  className?: string;
}) {
  return (
    <span
      aria-hidden="true"
      className={`flex shrink-0 items-center justify-center rounded-full text-[9px] font-semibold uppercase ${
        inverted
          ? "bg-background-base/20 text-background-base"
          : "bg-content/15 text-content/70"
      } ${className}`}
    >
      {host.charAt(0)}
    </span>
  );
}
