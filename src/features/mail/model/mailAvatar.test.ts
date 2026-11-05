// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import {
  PHOTO_EVENT,
  gravatarUrl,
  loadMyPhoto,
  organisationDomain,
  saveMyPhoto,
} from "./mailAvatar";

describe("the faces of a reply", () => {
  it("keeps the photo the user chose per address, and says so", () => {
    const changed = vi.fn();
    window.addEventListener(PHOTO_EVENT, changed);
    saveMyPhoto(" Ana@Ejemplo.test ", "data:image/png;base64,AAA");
    expect(loadMyPhoto("ana@ejemplo.test")).toBe("data:image/png;base64,AAA");
    saveMyPhoto("ana@ejemplo.test", undefined);
    expect(loadMyPhoto("ana@ejemplo.test")).toBeUndefined();
    expect(changed).toHaveBeenCalledTimes(2);
    window.removeEventListener(PHOTO_EVENT, changed);
  });

  it("asks Gravatar by the address's hash, never the address", async () => {
    const url = await gravatarUrl(" Olivia@Vercel.com ");
    expect(url).toMatch(/^https:\/\/gravatar\.com\/avatar\/[0-9a-f]{64}\?s=64&d=404$/);
    expect(url).toBe(await gravatarUrl("olivia@vercel.com"));
    expect(url).not.toContain("vercel");
    expect(await gravatarUrl("not an address")).toBeUndefined();
  });

  it("shows an organisation's icon, not a mail provider's", () => {
    expect(organisationDomain("thompson@ace.com")).toBe("ace.com");
    expect(organisationDomain("olivia@Vercel.com")).toBe("vercel.com");
    expect(organisationDomain("me@gmail.com")).toBeUndefined();
    expect(organisationDomain("me@hotmail.es")).toBeUndefined();
    expect(organisationDomain("me@icloud.com")).toBeUndefined();
    expect(organisationDomain("nobody")).toBeUndefined();
  });
});
