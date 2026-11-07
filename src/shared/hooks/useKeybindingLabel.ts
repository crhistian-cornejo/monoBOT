import { useSyncExternalStore } from "react";
import {
  keybindingShortcutLabel,
  subscribeKeybindings,
} from "../../features/settings/model/settings";

/**
 * The display label for a command's shortcut ("Ctrl+B", "⌘B"), re-read every
 * time the user's keybinding overrides change. Undefined while the command
 * is unbound, so tooltips and aria labels follow whatever is configured.
 */
export function useKeybindingLabel(
  command: string,
  fallback: string,
): string | undefined {
  const snapshot = () =>
    keybindingShortcutLabel(command, fallback) ?? undefined;
  return useSyncExternalStore(subscribeKeybindings, snapshot, snapshot);
}
