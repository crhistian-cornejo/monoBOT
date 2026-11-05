import { useEffect, useState, type ReactNode } from "react";
import { PersonAvatar } from "../../../shared/ui/PersonAvatar";
import { PrivateEmail } from "../../../shared/ui/PrivateEmail";
import { CitationMonogram } from "../../sessions/ui/CitationMonogram";
import { personLabel, type MailPerson } from "../model/mail";
import {
  PHOTO_EVENT,
  PHOTO_SIZE,
  gravatarUrl,
  loadMyPhoto,
  organisationDomain,
} from "../model/mailAvatar";

/**
 * The picture for an address: for one of the user's own accounts, the photo
 * they chose; else Gravatar's (the initial shows when it has none).
 */
export function useMailFace(address: string, own = false): string | undefined {
  const [photo, setPhoto] = useState(() => (own ? loadMyPhoto(address) : undefined));
  const [gravatar, setGravatar] = useState<string>();

  useEffect(() => {
    if (!own) return;
    const reload = () => setPhoto(loadMyPhoto(address));
    reload();
    window.addEventListener(PHOTO_EVENT, reload);
    return () => window.removeEventListener(PHOTO_EVENT, reload);
  }, [address, own]);

  useEffect(() => {
    let alive = true;
    setGravatar(undefined);
    void gravatarUrl(address)
      .then((url) => alive && setGravatar(url))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [address]);

  return photo ?? gravatar;
}

/** A person on a reply: their face, who they are, and their organisation's icon. */
export function MailPersonChip({ person, own = false }: { person: MailPerson; own?: boolean }) {
  const face = useMailFace(person.address, own);
  const label = personLabel(person);
  const named = label !== person.address;
  const domain = organisationDomain(person.address);
  return (
    <span className="flex min-w-0 items-center gap-1.5" title={person.address}>
      <PersonAvatar name={label} src={face} />
      {named ? <span className="shrink-0 truncate text-content/85">{label}</span> : null}
      <span className={`flex min-w-0 ${named ? "text-content/45" : "text-content/85"}`}>
        {own ? <PrivateEmail email={person.address} /> : <span className="truncate">{person.address}</span>}
      </span>
      {domain ? <CitationMonogram host={domain} className="size-4 ring-1 ring-content/10" /> : null}
    </span>
  );
}

/** One line of a reply's header: «From», «To» or «Cc», then its people. */
export function MailPeopleRow({
  label,
  children,
  end,
}: {
  label: string;
  children: ReactNode;
  /** Controls at the end of the line. */
  end?: ReactNode;
}) {
  return (
    <div className="flex min-h-9 min-w-0 items-center gap-2 px-2 py-1 text-[12px]">
      <span className="w-9 shrink-0 text-content/45">{label}</span>
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1">{children}</div>
      {end ? <div className="flex shrink-0 items-center gap-1">{end}</div> : null}
    </div>
  );
}

/** A picture the user picked, cropped to a square and kept small enough to store. */
export async function readPhoto(file: File): Promise<string> {
  const bitmap = await createImageBitmap(file);
  const side = Math.min(bitmap.width, bitmap.height);
  const size = Math.min(PHOTO_SIZE, side);
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("The picture could not be read.");
  context.drawImage(
    bitmap,
    (bitmap.width - side) / 2,
    (bitmap.height - side) / 2,
    side,
    side,
    0,
    0,
    size,
    size,
  );
  bitmap.close();
  return canvas.toDataURL("image/png");
}
