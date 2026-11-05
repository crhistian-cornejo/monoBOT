// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MailOverview } from "../model/mail";

const api = vi.hoisted(() => ({
  accounts: vi.fn(),
  overview: vi.fn(),
  last: vi.fn(),
  prefetch: vi.fn(),
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
  api.last.mockResolvedValue(null);
  api.prefetch.mockResolvedValue(undefined);
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

  it("paints the last reading at once, then the fresh one, and reads ahead", async () => {
    api.accounts.mockResolvedValue(overview.accounts);
    api.last.mockResolvedValue({
      ...overview,
      messages: [{ ...overview.messages[0], subject: "From the snapshot" }],
    });
    let fresh!: (value: MailOverview) => void;
    api.overview.mockReturnValue(new Promise((resolve) => (fresh = resolve)));
    act(() => root.render(createElement(MailInbox)));
    await flush();
    await flush();
    expect(container.textContent).toContain("From the snapshot");
    expect(api.prefetch).toHaveBeenCalledWith(["a.inbox.7"]);
    await act(async () => fresh(overview));
    expect(container.textContent).toContain("Planos de las pozas");
    expect(container.textContent).not.toContain("From the snapshot");
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

  it("loads a message's pictures as it opens, unless that is turned off", async () => {
    const open = async (id: string) => {
      const message = { ...overview.messages[0], id, subject: `Fotos ${id}` };
      api.accounts.mockResolvedValue(overview.accounts);
      api.overview.mockResolvedValue({ ...overview, messages: [message] });
      api.open.mockResolvedValue({
        id,
        html: '<img src="https://img.test/a.png">',
        text: [],
        remote: ["https://img.test/a.png", "https://img.test/b.png"],
        imagesAllowed: false,
        attachments: [],
      });
      api.image.mockResolvedValue("data:image/png;base64,AA");
      act(() => root.render(createElement(MailInbox)));
      await flush();
      await flush();
      const row = [...container.querySelectorAll("button")].find((button) =>
        button.textContent?.includes(`Fotos ${id}`),
      )!;
      act(() => row.click());
      await flush();
      await flush();
    };

    await open("a.inbox.8");
    expect(api.image).toHaveBeenCalledWith("a.inbox.8", "https://img.test/a.png");
    expect(api.image).toHaveBeenCalledWith("a.inbox.8", "https://img.test/b.png");

    act(() => root.unmount());
    root = createRoot(container);
    api.image.mockClear();
    localStorage.setItem("monocode.mail.autoImages", "0");
    try {
      await open("a.inbox.9");
      expect(api.image).not.toHaveBeenCalled();
      expect(container.textContent).toContain("Show 2 pictures from the web");
    } finally {
      localStorage.removeItem("monocode.mail.autoImages");
    }
  });
});
