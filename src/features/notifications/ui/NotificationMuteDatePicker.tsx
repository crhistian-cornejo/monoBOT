import {
  loadNotificationPreferences,
  updateNotificationPreferences,
} from "../model/notificationPreferences";
import { DateTimeForm } from "../../../shared/ui/DateTimeForm";

type Props = {
  projectIds: readonly string[];
  onChanged?: () => void;
  onCancel: () => void;
};

/** Custom timing is a complete step, separate from the duration presets. */
export function NotificationMuteDatePicker({
  projectIds,
  onChanged,
  onCancel,
}: Props) {
  const until =
    projectIds.length === 1
      ? loadNotificationPreferences()[projectIds[0]]?.mutedUntil
      : undefined;
  return (
    <DateTimeForm
      title="Mute all notifications until"
      submitLabel="Mute until then"
      initial={
        typeof until === "number" && until > Date.now()
          ? until
          : Date.now() + 3_600_000
      }
      disabled={!projectIds.length}
      onCancel={onCancel}
      onSubmit={(mutedUntil) => {
        try {
          updateNotificationPreferences(projectIds, { mutedUntil });
          onChanged?.();
        } catch {
          return "Could not save notification preferences. Please try again.";
        }
      }}
    />
  );
}
