import { useRef, useState } from "react";
import {
  ExplorerMenu,
  type ExplorerMenuItem,
} from "../../files/ui/ExplorerMenu";
import { DateTimeForm } from "../../../shared/ui/DateTimeForm";
import { Popover } from "../../../shared/ui/Popover";
import { Clock } from "../../../shared/ui/icons";
import { SEND_LATER_CUSTOM } from "../model/scheduledSend";
import { reminderTime } from "../model/sessionReminders";
import { sessionReminderPresets } from "./sessionReminderPresets";

const MENU_WIDTH = 250;

/** The same times as "Remind me", then any date and time. */
function sendLaterItems(): ExplorerMenuItem[] {
  return [
    ...sessionReminderPresets(),
    { kind: "sep" },
    { kind: "item", id: SEND_LATER_CUSTOM, label: "Choose date and time" },
  ];
}

/** "Send later": a preset time, or any date and time. */
export function SendLaterButton({
  onSchedule,
  onClose,
}: {
  onSchedule: (sendAt: number) => void;
  /** Hand focus back to the composer. */
  onClose?: () => void;
}) {
  const [open, setOpen] = useState<"menu" | "custom" | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [items, setItems] = useState(sendLaterItems);
  const close = () => {
    setOpen(null);
    onClose?.();
  };
  const rect = open === "menu" ? trigger.current?.getBoundingClientRect() : null;

  return (
    <>
      <button
        ref={trigger}
        type="button"
        title="Send later"
        aria-label="Send later"
        aria-haspopup={open === "custom" ? "dialog" : "menu"}
        aria-expanded={open !== null}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          // The labels show clock times; refresh them each time it opens.
          setItems(sendLaterItems());
          setOpen(open ? null : "menu");
        }}
        className="grid size-6.5 shrink-0 place-items-center rounded-md text-content/45 transition-colors hover:bg-content/8 hover:text-content/90"
      >
        <Clock className="size-3.5" />
      </button>
      {rect ? (
        <ExplorerMenu
          x={Math.max(8, rect.right - MENU_WIDTH)}
          y={rect.top - 4}
          ariaLabel="Send later"
          width={MENU_WIDTH}
          header={
            <p className="px-2 py-1.5 text-[11px] text-content/45">
              Send this message
            </p>
          }
          items={items}
          onClose={close}
          onPick={(id) => {
            if (id === SEND_LATER_CUSTOM) {
              setOpen("custom");
              return;
            }
            const sendAt = reminderTime(id);
            setOpen(null);
            if (sendAt != null) onSchedule(sendAt);
          }}
        />
      ) : null}
      {open === "custom" ? (
        <Popover
          anchor={trigger}
          align="end"
          side="top"
          width={280}
          role="dialog"
          aria-label="Send later"
          onDismiss={close}
          className="overflow-y-auto p-3"
        >
          <DateTimeForm
            title="Send this message on"
            submitLabel="Schedule"
            initial={Date.now() + 3_600_000}
            onCancel={close}
            onSubmit={(sendAt) => {
              setOpen(null);
              onSchedule(sendAt);
            }}
          />
        </Popover>
      ) : null}
    </>
  );
}
