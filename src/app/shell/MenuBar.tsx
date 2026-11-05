import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ExplorerMenu,
  type ExplorerMenuItem,
} from "../../features/files/ui/ExplorerMenu";
import { ALT, MOD, SHIFT } from "../../platform/tauri/platform";
import { runUpdateFlow } from "../model/updater";
import {
  keybindingShortcutLabel,
  loadAutosave,
  loadKeybindingOverrides,
  saveAutosave,
  subscribeAutosave,
  subscribeKeybindings,
} from "../../features/settings/model/settings";

type MenuKey = "file" | "view" | "terminal";

type Props = {
  onNew: () => void;
  onNewTerminal?: () => void;
  onToggleTerminal?: () => void;
  onGoToFile?: () => void;
  onToggleSidebar: () => void;
  onToggleSessionSidebar: () => void;
  onShowSourceControl?: () => void;
  onCloseCurrentTab?: () => void;
  onCloseOtherTabs?: () => void;
  onCloseAllTabs?: () => void;
  onPickProject?: () => void;
  onFindInProject?: () => void;
  onSearch?: () => void;
  onOpenInbox?: () => void;
  onOpenNotes?: () => void;
  onZoomIn?: () => void;
  onZoomOut?: () => void;
  onZoomReset?: () => void;
};

/** The File, View and Terminal menus, opened by a hamburger or by tapping Alt. */
export function AppMenu({
  onNew,
  onNewTerminal,
  onToggleTerminal,
  onGoToFile,
  onToggleSidebar,
  onToggleSessionSidebar,
  onShowSourceControl,
  onCloseCurrentTab,
  onCloseOtherTabs,
  onCloseAllTabs,
  onPickProject,
  onFindInProject,
  onSearch,
  onOpenInbox,
  onOpenNotes,
  onZoomIn,
  onZoomOut,
  onZoomReset,
}: Props) {
  const [anchor, setAnchor] = useState<MenuAnchor | null>(null);
  const [, refreshShortcuts] = useState(loadKeybindingOverrides);
  const [autosave, setAutosave] = useState(loadAutosave);

  // Pressing the hamburger while the menu is open first closes it as an
  // outside click; the click that follows must not open it again.
  const closedAt = useRef(0);

  // A hamburger asks for the menu under itself; a second click closes it.
  useEffect(() => {
    const onOpen = (event: Event) => {
      const at = (event as CustomEvent<MenuAnchor>).detail;
      if (Date.now() - closedAt.current < 300) return;
      setAnchor((open) => (open ? null : at));
    };
    window.addEventListener(OPEN_APP_MENU_EVENT, onOpen);
    return () => window.removeEventListener(OPEN_APP_MENU_EVENT, onOpen);
  }, []);

  useEffect(
    () =>
      subscribeKeybindings(() => refreshShortcuts(loadKeybindingOverrides())),
    [],
  );

  useEffect(
    () => subscribeAutosave(() => setAutosave(loadAutosave())),
    [],
  );

  const shortcut = (command: string, keys: string) =>
    keybindingShortcutLabel(command, keys) ?? undefined;

  // Toggle with standalone Alt key tap
  useEffect(() => {
    let altPressedAlone = false;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Alt") {
        altPressedAlone = true;
      } else if (altPressedAlone) {
        altPressedAlone = false;
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === "Alt" && altPressedAlone) {
        setAnchor((open) => (open ? null : visibleButtonAnchor()));
        altPressedAlone = false;
      }
    };

    const onBlur = () => {
      altPressedAlone = false;
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  const closeMenu = useCallback(() => {
    closedAt.current = Date.now();
    setAnchor(null);
  }, []);

  const handlePick = useCallback(
    (id: string) => {
      closeMenu();

      switch (id) {
        case "new_tab":
          onNew();
          break;
        case "new_terminal":
          onNewTerminal?.();
          break;
        case "toggle_terminal":
          onToggleTerminal?.();
          break;
        case "new_window":
          void invoke("open_new_window").catch(() => {});
          break;
        case "open_project":
          onPickProject?.();
          break;
        case "open_search":
          onSearch?.();
          break;
        case "open_inbox":
          onOpenInbox?.();
          break;
        case "open_notes":
          onOpenNotes?.();
          break;
        case "go_to_file":
          onGoToFile?.();
          break;
        case "find_in_project":
          onFindInProject?.();
          break;
        case "close_tab":
          onCloseCurrentTab?.();
          break;
        case "close_other_tabs":
          onCloseOtherTabs?.();
          break;
        case "close_all_tabs":
          onCloseAllTabs?.();
          break;
        case "toggle_autosave": {
          const next = saveAutosave(!loadAutosave());
          setAutosave(next);
          break;
        }
        case "toggle_sidebar":
          onToggleSidebar();
          break;
        case "toggle_session_sidebar":
          onToggleSessionSidebar();
          break;
        case "open_model_picker":
          window.dispatchEvent(new Event("open_model_picker"));
          break;
        case "toggle_diff":
          onShowSourceControl?.();
          break;
        case "check_for_updates":
          void runUpdateFlow(true);
          break;
        case "zoom_in":
          onZoomIn?.();
          break;
        case "zoom_out":
          onZoomOut?.();
          break;
        case "zoom_reset":
          onZoomReset?.();
          break;
      }
    },
    [
      closeMenu,
      autosave,
      onCloseCurrentTab,
      onCloseOtherTabs,
      onCloseAllTabs,
      onFindInProject,
      onGoToFile,
      onNew,
      onNewTerminal,
      onToggleTerminal,
      onPickProject,
      onSearch,
      onOpenInbox,
      onOpenNotes,
      onShowSourceControl,
      onToggleSidebar,
      onToggleSessionSidebar,
      onZoomIn,
      onZoomOut,
      onZoomReset,
    ],
  );

  const getMenuItems = (key: MenuKey): ExplorerMenuItem[] => {
    switch (key) {
      case "file":
        return [
          {
            kind: "item",
            id: "new_tab",
            label: "New Tab",
            shortcut: shortcut("Tab: New", `${MOD}T`),
          },
          {
            kind: "item",
            id: "new_terminal",
            label: "New Terminal",
            shortcut: shortcut("Terminal: New", `${MOD}\``),
          },
          {
            kind: "item",
            id: "new_window",
            label: "New Window",
            shortcut: shortcut("App: New Window", `${MOD}${SHIFT}N`),
          },
          { kind: "sep" },
          {
            kind: "item",
            id: "toggle_autosave",
            label: "Autosave",
            checked: autosave,
          },
          { kind: "sep" },
          {
            kind: "item",
            id: "open_project",
            label: "Open Project…",
            shortcut: shortcut("App: Open Project", `${MOD}O`),
          },
          {
            kind: "item",
            id: "open_search",
            label: "Search…",
            shortcut: shortcut("App: Search", `${MOD}K`),
          },
          {
            kind: "item",
            id: "go_to_file",
            label: "Go to File…",
            shortcut: shortcut("App: Go to File", `${MOD}P`),
          },
          {
            kind: "item",
            id: "find_in_project",
            label: "Find in Files…",
            shortcut: shortcut("App: Find in Files", `${MOD}${SHIFT}F`),
          },
          { kind: "sep" },
          {
            kind: "item",
            id: "close_tab",
            label: "Close Pane",
            shortcut: shortcut("Pane: Close", `${MOD}W`),
          },
          {
            kind: "item",
            id: "close_other_tabs",
            label: "Close Other Tabs",
            shortcut: shortcut("Tab: Close Others", `${MOD}${ALT}T`),
          },
          {
            kind: "item",
            id: "close_all_tabs",
            label: "Close All Tabs",
            shortcut: shortcut("Tab: Close All", `${MOD}${SHIFT}W`),
          },
          { kind: "sep" },
          {
            kind: "item",
            id: "check_for_updates",
            label: "Check for Updates…",
          },
        ];
      case "view":
        return [
          {
            kind: "item",
            id: "toggle_sidebar",
            label: "Toggle Sidebar",
            shortcut: shortcut("App: Toggle Sidebar", `${MOD}B`),
          },
          {
            kind: "item",
            id: "toggle_session_sidebar",
            label: "Toggle Session Sidebar",
            shortcut: shortcut(
              "App: Toggle Session Sidebar",
              `${MOD}${SHIFT}B`,
            ),
          },
          { kind: "item", id: "open_inbox", label: "Inbox" },
          ...(onOpenNotes
            ? [{ kind: "item" as const, id: "open_notes", label: "Notes" }]
            : []),
          {
            kind: "item",
            id: "toggle_terminal",
            label: "Toggle Terminal",
            shortcut: shortcut("Terminal: Toggle Dock", `${MOD}J`),
          },
          {
            kind: "item",
            id: "open_model_picker",
            label: "Switch Model…",
            shortcut: shortcut("App: Switch Model", `${MOD}.`),
          },
          { kind: "item", id: "toggle_diff", label: "Toggle Changes" },
          { kind: "sep" },
          {
            kind: "item",
            id: "zoom_in",
            label: "Zoom In",
            shortcut: shortcut("View: Zoom In", `${MOD}+`),
          },
          {
            kind: "item",
            id: "zoom_out",
            label: "Zoom Out",
            shortcut: shortcut("View: Zoom Out", `${MOD}-`),
          },
          {
            kind: "item",
            id: "zoom_reset",
            label: "Reset Zoom",
            shortcut: shortcut("View: Reset Zoom", `${MOD}0`),
          },
        ];
      case "terminal":
        return [
          {
            kind: "item",
            id: "new_terminal",
            label: "New Terminal",
            shortcut: shortcut("Terminal: New", `${MOD}\``),
          },
          {
            kind: "item",
            id: "toggle_terminal",
            label: "Toggle Terminal",
            shortcut: shortcut("Terminal: Toggle Dock", `${MOD}J`),
          },
        ];
    }
  };

  if (!anchor) return null;

  // A hamburger has no room for a bar, so File, View and Terminal open to the side.
  const MENUS: { key: MenuKey; label: string }[] = [
    { key: "file", label: "File" },
    { key: "view", label: "View" },
    { key: "terminal", label: "Terminal" },
  ];
  return (
    <ExplorerMenu
      x={anchor.x}
      y={anchor.y}
      width={180}
      ariaLabel="Application menu"
      items={MENUS.map(({ key, label }) => ({
        kind: "item" as const,
        id: `menu:${key}`,
        label,
        submenu: getMenuItems(key).flatMap((item) =>
          item.kind === "item" ? [item] : [],
        ),
      }))}
      onPick={handlePick}
      onClose={closeMenu}
    />
  );
}

type MenuAnchor = { x: number; y: number };

const OPEN_APP_MENU_EVENT = "monocode:open-app-menu";

/** Ask the one AppMenu to open under `button`, or to close if it's open. */
export function openAppMenu(button: HTMLElement) {
  const rect = button.getBoundingClientRect();
  window.dispatchEvent(
    new CustomEvent<MenuAnchor>(OPEN_APP_MENU_EVENT, {
      detail: { x: rect.left, y: rect.bottom + 4 },
    }),
  );
}

/** Alt opens the menu under whichever hamburger is on screen. */
function visibleButtonAnchor(): MenuAnchor {
  const button = [
    ...document.querySelectorAll<HTMLElement>("[data-app-menu-button]"),
  ].find((element) => element.getClientRects().length > 0);
  if (!button) return { x: 8, y: 44 };
  const rect = button.getBoundingClientRect();
  return { x: rect.left, y: rect.bottom + 4 };
}
