import { describe, expect, it } from "vitest";
import {
  keepFor,
  mayRead,
  onMailConnectRequest,
  openToAll,
  parseMailAccess,
  providerFor,
  readersOf,
  requestMailConnect,
} from "./mailAccess";

describe("mail access", () => {
  it("leaves an account open to every agent until a bot keeps it", () => {
    expect(mayRead({}, "me@gmail.com", "finance")).toBe(true);
    expect(mayRead({}, "me@gmail.com", undefined)).toBe(true);
  });

  it("keeps an account for the bot that connected it alone", () => {
    const access = keepFor({}, "Ana@Gmail.com ", "finance");
    expect(mayRead(access, "ana@gmail.com", "finance")).toBe(true);
    expect(mayRead(access, "ana@gmail.com", "inbox")).toBe(false);
    expect(mayRead(access, "ana@gmail.com", undefined)).toBe(false);
    expect(readersOf(access, "ANA@gmail.com")).toEqual(["finance"]);
  });

  it("adds a second reader once and opens to all on request", () => {
    let access = keepFor({}, "ana@gmail.com", "finance");
    access = keepFor(access, "ana@gmail.com", "finance");
    access = keepFor(access, "ana@gmail.com", "planner");
    expect(readersOf(access, "ana@gmail.com")).toEqual(["finance", "planner"]);
    expect(readersOf(openToAll(access, "ANA@gmail.com"), "ana@gmail.com")).toBeUndefined();
  });

  it("drops what it does not understand when loading", () => {
    expect(
      parseMailAccess({ "A@x.test": ["one", 2, ""], "b@x.test": "one", c: [] }),
    ).toEqual({ "a@x.test": ["one"] });
    expect(parseMailAccess([])).toEqual({});
    expect(parseMailAccess(null)).toEqual({});
  });

  it("guesses the provider from the domain", () => {
    expect(providerFor("ana@gmail.com")).toBe("gmail");
    expect(providerFor("ana@me.com")).toBe("icloud");
    expect(providerFor("ana@empresa.pe")).toBe("imap");
  });

  it("says whether a dialog took the request", () => {
    const request = { address: "ana@gmail.com", provider: "gmail" as const, askedBy: "Finance" };
    expect(requestMailConnect(request)).toBe(false);
    const seen: string[] = [];
    const stop = onMailConnectRequest((r) => seen.push(r.address));
    expect(requestMailConnect(request)).toBe(true);
    stop();
    expect(seen).toEqual(["ana@gmail.com"]);
  });
});
