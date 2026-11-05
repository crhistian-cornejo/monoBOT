// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MailOverview } from "../model/mail";

const api = vi.hoisted(() => ({
  accounts: vi.fn(),
  overview: vi.fn(),
  open: vi.fn(),
  act: vi.fn(async () => undefined),
  addAccount: vi.fn(),
  removeAccount: vi.fn(),
  archiveMore: vi.fn(),
  interesting: vi.fn(),
  draft: vi.fn(),
  send: vi.fn(),
  image: vi.fn(),
  attachment: vi.fn(),
}));
vi.mock("../model/mailApi", () => ({
  mailApi: api,
  onMailChanged: vi.fn(async () => () => undefined),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const { MailInbox } = await import("./MailInbox");

const overview: MailOverview = {
  accounts: [{ id: "a", name: "Work", address: "me@mina.pe", provider: "gmail" }],
  problems: [],
  totals: [],
  messages: [
    {
      id: "a.inbox.7",
      accountId: "a",
      folder: "inbox",
      from: "Ana Pérez",
      address: "ana@mina.pe",
      subject: "Planos de las pozas",
      preview: "Te envío los planos revisados",
      body: ["Te envío los planos revisados."],
      at: Date.now() - 60_000,
      unread: true,
      starred: false,
      triage: "routine",
      labels: [],
    },
  ],
};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  Object.values(api).forEach((fn) => fn.mockReset());
  api.act.mockResolvedValue(undefined);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const flush = () => act(async () => await new Promise((resolve) => setTimeout(resolve, 0)));

describe("MailInbox", () => {
  it("explains how to connect an account when there is none", async () => {
    api.accounts.mockResolvedValue([]);
    act(() => root.render(createElement(MailInbox)));
    await flush();
    expect(container.textContent).toContain("Your email in the Inbox");
    expect(container.textContent).toContain("Create a Gmail app password");
    expect(api.overview).not.toHaveBeenCalled();
  });

  it("lists the mail, and opening a message marks it read", async () => {
    api.accounts.mockResolvedValue(overview.accounts);
    api.overview.mockResolvedValue(overview);
    api.open.mockResolvedValue({
      id: "a.inbox.7",
      html: "",
      text: ["Te envío los planos revisados."],
      remote: [],
      imagesAllowed: false,
      attachments: [],
    });
    act(() => root.render(createElement(MailInbox)));
    await flush();
    await flush();
    expect(api.overview).toHaveBeenCalledWith(false);
    const row = [...container.querySelectorAll("button")].find((button) =>
      button.textContent?.includes("Planos de las pozas"),
    )!;
    expect(row.textContent).toContain("Ana Pérez");
    act(() => row.click());
    await flush();
    expect(api.act).toHaveBeenCalledWith("a.inbox.7", "read");
    expect(api.open).toHaveBeenCalledWith("a.inbox.7");
    expect(container.textContent).toContain("Reply (R)");
    expect(container.querySelector('[aria-label="Unread"]')).toBeNull();
  });
});
