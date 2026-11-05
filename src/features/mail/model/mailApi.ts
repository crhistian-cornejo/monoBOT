import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  MailAccount,
  MailAction,
  MailOverview,
  MailRecipients,
  MailSearchResult,
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
  /**
   * The whole history of the accounts in `addresses` (all when unset), with
   * Gmail's search words; read only. Each account says what it found.
   */
  search: (
    query: string,
    options: { addresses?: string[]; limit: number; oldestFirst: boolean },
  ) =>
    invoke<MailSearchResult[]>("mail_search", {
      query,
      addresses: options.addresses ?? null,
      limit: options.limit,
      oldestFirst: options.oldestFirst,
    }),
  overview: (force = false) => invoke<MailOverview>("mail_overview", { force }),
  /** The last reading at once (memory, else the snapshot on disk); null before the first. */
  last: () => invoke<MailOverview | null>("mail_last"),
  /** Reads ahead what the user will likely open; nothing is marked read. */
  prefetch: (ids: string[]) => invoke<void>("mail_prefetch", { ids }),
  archiveMore: () => invoke<MailOverview>("mail_archive_more"),
  open: (id: string) => invoke<OpenedMail>("mail_open", { id }),
  act: (id: string, action: MailAction) =>
    invoke<void>("mail_act", { id, action }),
  interesting: (address: string, on: boolean) =>
    invoke<void>("mail_interesting", { address, on }),
  /**
   * Who a reply goes to: whom the message asks replies to go to, else its
   * sender; with `all`, a copy to everyone else it went to. Only Rust decides.
   */
  recipients: (id: string, all: boolean) =>
    invoke<MailRecipients>("mail_recipients", { id, all }),
  /** `subject` is the one the user chose; without it, Rust sends «Re: » and theirs. */
  draft: (id: string, text: string, all = false, subject?: string) =>
    invoke<void>("mail_draft", { id, text, all, subject: subject?.trim() || null }),
  /** Only the user's own press on Send calls this. */
  send: (id: string, text: string, all = false, subject?: string) =>
    invoke<void>("mail_send", { id, text, all, subject: subject?.trim() || null }),
  image: (id: string, url: string) => invoke<string>("mail_image", { id, url }),
  attachment: (id: string, index: number) =>
    invoke<string>("mail_attachment", { id, index }),
};

/** New mail, or a change made elsewhere (read on the phone): read again. */
export function onMailChanged(handler: (arrived: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("mail-changed", (event) => handler(event.payload));
}
