import { useEffect, useState } from "react";
import { cachedSiteIcon, fetchSiteIcon } from "../model/linkPreview";

/**
 * A site's icon (its favicon), read once per site. The first letter of the
 * host stands in while it loads, and for a site that has none.
 */
export function CitationMonogram({
  host,
  inverted = false,
  className = "size-4",
}: {
  /** The site's host, or a full URL. */
  host: string;
  /** On the filled (active) chip, where the ink and the ground swap. */
  inverted?: boolean;
  className?: string;
}) {
  const [icon, setIcon] = useState(() => cachedSiteIcon(host) ?? null);
  const [broken, setBroken] = useState(false);
  useEffect(() => {
    let live = true;
    setBroken(false);
    const known = cachedSiteIcon(host);
    if (known !== undefined) {
      setIcon(known);
      return;
    }
    setIcon(null);
    void fetchSiteIcon(host).then((next) => {
      if (live) setIcon(next);
    });
    return () => {
      live = false;
    };
  }, [host]);

  if (icon && !broken)
    return (
      <span
        aria-hidden="true"
        className={`flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-background-base ${className}`}
      >
        <img
          src={icon}
          alt=""
          draggable={false}
          onError={() => setBroken(true)}
          className="size-[75%] object-contain"
        />
      </span>
    );

  return (
    <span
      aria-hidden="true"
      className={`flex shrink-0 items-center justify-center rounded-full text-[9px] font-semibold uppercase ${
        inverted
          ? "bg-background-base/20 text-background-base"
          : "bg-content/15 text-content/70"
      } ${className}`}
    >
      {letter(host)}
    </span>
  );
}

/** The host's first letter; a full URL's, past its scheme. */
function letter(host: string): string {
  const bare = host.replace(/^[a-z]+:\/\//i, "").replace(/^www\./i, "");
  return bare.charAt(0);
}
