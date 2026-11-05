// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MailAttachment } from "../model/mail";
import { MailAttachments } from "./MailAttachments";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const file = (patch: Partial<MailAttachment>): MailAttachment => ({
  index: 0,
  name: "x",
  kind: "application/octet-stream",
  size: 2048,
  risk: { level: "safe", reasons: [] },
  ...patch,
});

describe("MailAttachments", () => {
  it("shows pictures in place and says what was found in each file", () => {
    const onOpen = vi.fn();
    const files = [
      file({ index: 0, name: "foto.png", preview: "data:image/png;base64,AA" }),
      file({ index: 1, name: "contrato.pdf" }),
      file({
        index: 2,
        name: "docs.zip",
        risk: { level: "caution", reasons: ["It has a password, so what is inside can't be checked."] },
      }),
      file({ index: 3, name: "factura.pdf.exe", risk: { level: "danger", reasons: ["It is a Windows program."] } }),
    ];
    act(() => root.render(createElement(MailAttachments, { attachments: files, onOpen })));

    const picture = container.querySelector("img")!;
    expect(picture.getAttribute("src")).toBe("data:image/png;base64,AA");
    const cards = [...container.querySelectorAll("li")].filter((li) => li.querySelector("svg"));
    const card = (name: string) => cards.find((li) => li.textContent?.includes(name))!;
    expect(card("contrato.pdf").textContent).toContain("Checked");
    expect(card("docs.zip").textContent).toContain("Careful");
    expect(card("docs.zip").textContent).toContain("password");
    expect(card("factura.pdf.exe").textContent).toContain("Blocked");
    expect(card("factura.pdf.exe").querySelector("button")).toBeNull();

    act(() => card("contrato.pdf").querySelector("button")!.click());
    expect(onOpen).toHaveBeenCalledWith(files[1]);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });
});
