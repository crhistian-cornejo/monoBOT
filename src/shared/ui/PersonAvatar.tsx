import { useEffect, useState } from "react";

/**
 * A person's picture in a circle, or their initial while there is none or it
 * fails to load (a 404 from Gravatar, a broken link).
 */
export function PersonAvatar({
  name,
  src,
  className = "size-5 text-[10px]",
}: {
  name: string;
  src?: string;
  /** Its size, and the initial's text size. */
  className?: string;
}) {
  const [failed, setFailed] = useState(!src);
  const initial = name.trim().charAt(0).toUpperCase() || "?";

  useEffect(() => {
    setFailed(!src);
  }, [src]);

  return src && !failed ? (
    <img
      src={src}
      alt=""
      referrerPolicy="no-referrer"
      draggable={false}
      onError={() => setFailed(true)}
      className={`shrink-0 rounded-full bg-content/10 object-cover ${className}`}
    />
  ) : (
    <span
      aria-hidden
      className={`grid shrink-0 place-items-center rounded-full bg-content/12 font-medium text-content/55 ${className}`}
    >
      {initial}
    </span>
  );
}
