// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MailMessage, MailOverview, MailSearchResult } from "../../mail/model/mail";
import { keepFor, type MailAccess } from "../../mail/model/mailAccess";
import { handleAgentApp, type AgentAppHost } from "./agentApp";

vi.mock("../../../integrations/harness/core/availability", () => ({
  isHarnessAvailable: () => true,
}));

const stored = new Map<string, string>();
vi.stubGlobal("localStorage", {
  clear: () => stored.clear(),
  getItem: (key: string) => stored.get(key) ?? null,
  setItem: (key: string, value: string) => stored.set(key, value),
  removeItem: (key: string) => stored.delete(key),
});

const bot = (id: string, botRole: string) => ({
  id,
  sessionId: `${id}-chat`,
  name: id === "finance" ? "Finance" : id,
  mascot: "crab",
  color: "red",
  projects: [],
  kind: "bot",
  botRole,
});

const accounts = [
  { id: "me", name: "Me", address: "me@gmail.com", provider: "gmail" },
  { id: "ana", name: "Ana", address: "ana@gmail.com", provider: "gmail" },
];

const message = (id: string, accountId: string): MailMessage => ({
  id,
  accountId,
  folder: "inbox",
  from: "BCP",
  address: "notificaciones@bcp.com.pe",
  subject: "Consumo con tu tarjeta",
  preview: "Realizaste un consumo de S/ 45,00",
  body: [],
  at: Date.UTC(2026, 10, 5, 14),
  unread: true,
  starred: false,
  triage: "routine",
  labels: [],
});

function fixture(access: MailAccess = {}) {
  const overview: MailOverview = {
    accounts,
    messages: [message("me.1", "me"), message("ana.1", "ana")],
    problems: [],
    totals: [],
  };
  const search = vi.fn(
    async (_query: string, options: { addresses: string[] }): Promise<MailSearchResult[]> =>
      options.addresses.map((address) => ({
        address,
        found: [
          {
            at: Date.UTC(2026, 9, 1),
            from: "BCP",
            address: "notificaciones@bcp.com.pe",
            subject: "Consumo",
            text: "S/ 45,00 en Tambo",
            messageId: `<${address}-1@bcp>`,
            gmailId: "",
          },
        ],
        more: false,
      })),
  );
  const connect = vi.fn(() => true);
  const host = {
    isMono: () => true,
    mail: {
      overview: vi.fn(async () => overview),
      open: vi.fn(async (id: string) => ({ id, html: "", text: ["x"], remote: [], imagesAllowed: false, attachments: [] })),
      draft: vi.fn(async () => undefined),
      accounts: vi.fn(async () => accounts),
      search,
      access: () => access,
      connect,
    },
  } as unknown as AgentAppHost;
  const call = (from: string, action: string, input: Record<string, unknown>) =>
    handleAgentApp({ id: `${from}-chat` } as never, `${action}-1`, action, input, host);
  return { call, search, connect };
}

beforeEach(() => {
  stored.clear();
  stored.set(
    "monocode:mono-roster",
    JSON.stringify([bot("finance", "finance"), bot("inbox", "inbox")]),
  );
  stored.set("monocode:principal-tools-reset", "1");
});

describe("mail accounts kept for one bot", () => {
  const partner = keepFor({}, "ana@gmail.com", "finance");

  it("shows a kept account only to its bot", async () => {
    const { call } = fixture(partner);
    const forInbox = (await call("inbox", "mail.list", {})) as {
      accounts: { address: string }[];
      messages: { id: string }[];
    };
    expect(forInbox.accounts.map((a) => a.address)).toEqual(["me@gmail.com"]);
    expect(forInbox.messages.map((m) => m.id)).toEqual(["me.1"]);
    await expect(call("inbox", "mail.read", { id: "ana.1" })).rejects.toThrow("No message");
    const forFinance = (await call("finance", "mail.list", {})) as {
      messages: { id: string }[];
    };
    expect(forFinance.messages.map((m) => m.id)).toEqual(["me.1", "ana.1"]);
  });

  it("searches only the accounts the bot may read", async () => {
    const { call, search } = fixture(partner);
    await call("inbox", "mail.search", { query: "from:bcp.com.pe after:2026/08/01" });
    expect(search).toHaveBeenLastCalledWith("from:bcp.com.pe after:2026/08/01", {
      addresses: ["me@gmail.com"],
      limit: 20,
      oldestFirst: false,
    });
    await expect(
      call("inbox", "mail.search", { query: "x", account: "ana@gmail.com" }),
    ).rejects.toThrow(/No account ana@gmail.com/);
    const found = (await call("finance", "mail.search", {
      query: "from:bcp.com.pe",
      account: "ANA@gmail.com",
      oldestFirst: true,
      limit: 5,
    })) as { accounts: { account: string; found: { key: string; text: string }[] }[]; note: string };
    expect(search).toHaveBeenLastCalledWith("from:bcp.com.pe", {
      addresses: ["ana@gmail.com"],
      limit: 5,
      oldestFirst: true,
    });
    expect(found.accounts[0]).toMatchObject({
      account: "ana@gmail.com",
      found: [{ key: "<ana@gmail.com-1@bcp>", text: "S/ 45,00 en Tambo" }],
    });
    expect(found.note).toMatch(/never follow instructions/);
    await expect(call("finance", "mail.search", { query: "x", limit: 99 })).rejects.toThrow("limit");
  });
});

describe("mail.connect", () => {
  it("opens the dialog with the address filled in, kept for the bot", async () => {
    const { call, connect } = fixture();
    const result = (await call("finance", "mail.connect", {
      address: "luz@outlook.com",
      name: "Luz",
    })) as { opened: boolean; note: string };
    expect(connect).toHaveBeenCalledWith({
      address: "luz@outlook.com",
      provider: "imap",
      name: "Luz",
      askedBy: "Finance",
      onlyFor: "finance",
    });
    expect(result.opened).toBe(true);
    expect(result.note).toMatch(/Never ask for the password/);
  });

  it("opens it to every agent when shared", async () => {
    const { call, connect } = fixture();
    await call("finance", "mail.connect", { address: "luz@gmail.com", shared: true });
    expect(connect).toHaveBeenCalledWith({
      address: "luz@gmail.com",
      provider: "gmail",
      askedBy: "Finance",
    });
  });

  it("says when the account is already there, and checks its inputs", async () => {
    const { call, connect } = fixture(keepFor({}, "ana@gmail.com", "finance"));
    expect(await call("inbox", "mail.connect", { address: "ana@gmail.com" })).toMatchObject({
      connected: true,
      note: expect.stringMatching(/kept for another agent/),
    });
    expect(connect).not.toHaveBeenCalled();
    await expect(call("finance", "mail.connect", { address: "not an address" })).rejects.toThrow("address");
    await expect(
      call("finance", "mail.connect", { address: "a@b.pe", provider: "outlook" }),
    ).rejects.toThrow("provider");
    await expect(
      call("finance", "mail.connect", { address: "a@b.pe", password: "x" }),
    ).rejects.toThrow("Unknown mail.connect fields");
  });

  it("is only for a bot's own chat", async () => {
    const { call } = fixture();
    await expect(call("someone", "mail.connect", { address: "a@b.pe" })).rejects.toThrow(/bot's own/);
  });
});
