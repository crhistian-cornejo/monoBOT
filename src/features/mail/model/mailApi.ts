import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  MailAccount,
  MailAction,
  MailOverview,
  OpenedMail,
} from "./mail";

/** The calls to src-tauri/src/mail.rs. */
export const mailApi = {
  accounts: () => invoke<MailAccount[]>("mail_accounts"),
  addAccount: (input: {
    address: string;
    password: string;
    name: string;
    provider: "gmail" | "icloud" | "imap";
    host?: string;
  }) =>
    invoke<void>("mail_add_account", {
      address: input.address,
      password: input.password,
      name: input.name,
      provider: input.provider,
      host: input.host ?? null,
    }),
  removeAccount: (id: string) => invoke<void>("mail_remove_account", { id }),
  overview: (force = false) => invoke<MailOverview>("mail_overview", { force }),
  archiveMore: () => invoke<MailOverview>("mail_archive_more"),
  open: (id: string) => invoke<OpenedMail>("mail_open", { id }),
  act: (id: string, action: MailAction) =>
    invoke<void>("mail_act", { id, action }),
  interesting: (address: string, on: boolean) =>
    invoke<void>("mail_interesting", { address, on }),
  draft: (id: string, text: string) => invoke<void>("mail_draft", { id, text }),
  /** Only the user's own press on Send calls this. */
  send: (id: string, text: string) => invoke<void>("mail_send", { id, text }),
  image: (id: string, url: string) => invoke<string>("mail_image", { id, url }),
  attachment: (id: string, index: number) =>
    invoke<string>("mail_attachment", { id, index }),
};

/** New mail, or a change made elsewhere (read on the phone): read again. */
export function onMailChanged(handler: (arrived: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("mail-changed", (event) => handler(event.payload));
}
