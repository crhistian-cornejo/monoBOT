import { PixelMascot } from "../../projects/ui/PixelMascot";
import { MonoStatusDot } from "../../monos/ui/MonoRailMascot";
import { monoLook, type MonoStatus } from "../../monos/model/mono";
import type { Group } from "../model/group";
import { groupMembers } from "../model/groupBrief";

/** How many circles a stack shows, the count of the rest among them. */
const SHOWN = 3;

/**
 * A group's members as overlapping round avatars, the lead first, with the
 * rest counted at the end: who is in the room, at a glance. The group's own
 * state sits on the lead, as a Mono's sits on its mascot.
 */
export function GroupAvatarStack({
  group,
  status = "idle",
  size = "size-5",
}: {
  group: Group;
  status?: MonoStatus;
  /** Tailwind size of each avatar. */
  size?: string;
}) {
  const members = groupMembers(group).sort(
    (a, b) =>
      Number(b.botRole === "principal") - Number(a.botRole === "principal"),
  );
  // Three circles at most: past three members, the third counts the rest.
  const shown = members.slice(
    0,
    members.length > SHOWN ? SHOWN - 1 : SHOWN,
  );
  const more = members.length - shown.length;
  if (!shown.length) return null;
  return (
    <span aria-hidden className="relative flex shrink-0 -space-x-1.5">
      {shown.map((mono, index) => {
        const look = monoLook(mono);
        return (
          <span
            key={mono.id}
            style={{ zIndex: SHOWN - index }}
            className={`relative grid ${size} shrink-0 place-items-center rounded-full bg-background-base ring-2 ring-background-base`}
          >
            <span className="grid size-full place-items-center rounded-full bg-content/[0.07]">
              <PixelMascot
                name={look.mascot}
                color={look.color}
                still
                className="size-3.5"
              />
            </span>
          </span>
        );
      })}
      {more > 0 ? (
        <span
          className={`relative grid ${size} shrink-0 place-items-center rounded-full bg-background-base ring-2 ring-background-base`}
        >
          <span className="grid size-full place-items-center rounded-full bg-content/[0.07] text-[9px] font-medium tabular-nums text-content/70">
            +{more}
          </span>
        </span>
      ) : null}
      <MonoStatusDot
        status={status}
        color={monoLook(shown[0]).color}
        className="-right-0.5 -top-0.5 z-10 size-1.5"
      />
    </span>
  );
}
