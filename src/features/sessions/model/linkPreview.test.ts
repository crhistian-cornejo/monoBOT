import { describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  cachedSiteIcon,
  fetchSiteIcon,
  parseStandaloneHttpUrl,
  parseUserMessageLink,
  siteIconHost,
} from "./linkPreview";

describe("parseStandaloneHttpUrl", () => {
  it("normalizes a standalone web URL for display", () => {
    expect(
      parseStandaloneHttpUrl(
        "  https://www.example.com/docs/start?q=one#intro  ",
      ),
    ).toEqual({
      url: "https://www.example.com/docs/start?q=one#intro",
      host: "example.com",
      displayUrl: "example.com/docs/start?q=one#intro",
    });
  });

  it("keeps a root URL compact", () => {
    expect(parseStandaloneHttpUrl("http://example.com/")?.displayUrl).toBe(
      "example.com",
    );
  });

  it("does not turn prose or credentialed URLs into preview cards", () => {
    expect(
      parseStandaloneHttpUrl("take a look at https://example.com"),
    ).toBeNull();
    expect(
      parseStandaloneHttpUrl("https://person:secret@example.com"),
    ).toBeNull();
    expect(parseStandaloneHttpUrl("file:///tmp/example.html")).toBeNull();
  });
});

describe("parseUserMessageLink", () => {
  it("extracts a URL followed by a comment", () => {
    expect(
      parseUserMessageLink(
        "https://github.com/hardbeat920/monocode/pull/226 check this",
      ),
    ).toEqual({
      link: {
        url: "https://github.com/hardbeat920/monocode/pull/226",
        host: "github.com",
        displayUrl: "github.com/hardbeat920/monocode/pull/226",
        githubWorkItem: {
          kind: "pr",
          repo: "hardbeat920/monocode",
          number: 226,
        },
      },
      beforeText: "",
      afterText: " check this",
    });
  });

  it("recognizes GitHub issues and links to a PR subpage", () => {
    expect(
      parseUserMessageLink(
        "https://github.com/acme/widgets/issues/42#issuecomment-1",
      )?.link.githubWorkItem,
    ).toEqual({ kind: "issue", repo: "acme/widgets", number: 42 });
    expect(
      parseUserMessageLink("https://www.github.com/acme/widgets/pull/73/files")
        ?.link.githubWorkItem,
    ).toEqual({ kind: "pr", repo: "acme/widgets", number: 73 });
  });

  it("leaves other GitHub URLs as normal web links", () => {
    expect(
      parseUserMessageLink("https://github.com/acme/widgets/actions")?.link
        .githubWorkItem,
    ).toBeUndefined();
    expect(
      parseUserMessageLink("https://github.com/acme/widgets/issues/0")?.link
        .githubWorkItem,
    ).toBeUndefined();
  });

  it("preserves prose around a URL and drops sentence punctuation", () => {
    const result = parseUserMessageLink(
      "Please review (https://example.com/docs), thanks",
    );
    expect(result?.beforeText).toBe("Please review (");
    expect(result?.afterText).toBe("), thanks");
    expect(result?.link.url).toBe("https://example.com/docs");
  });

  it("returns null when there is no valid web URL", () => {
    expect(parseUserMessageLink("Nothing to preview here")).toBeNull();
  });
});

describe("site icons", () => {
  it("names the host a source's icon belongs to", () => {
    expect(siteIconHost("www.dnr.alaska.gov")).toBe("dnr.alaska.gov");
    expect(siteIconHost("https://www2.gov.bc.ca/a/b.pdf")).toBe("www2.gov.bc.ca");
    expect(siteIconHost("localhost")).toBeNull();
    expect(siteIconHost("file:///etc/passwd")).toBeNull();
    expect(siteIconHost("")).toBeNull();
  });

  it("reads each site's icon once, from its home page, failures included", async () => {
    invoke.mockReset();
    invoke.mockImplementation(async (_command: string, { url }: { url: string }) => {
      if (url === "https://broken.example/") throw new Error("offline");
      return { title: null, faviconDataUrl: `data:image/png;base64,${url}` };
    });
    const [one, two] = await Promise.all([
      fetchSiteIcon("doi.org"),
      fetchSiteIcon("https://doi.org/10.1/x"),
    ]);
    expect(one).toBe("data:image/png;base64,https://doi.org/");
    expect(two).toBe(one);
    expect(cachedSiteIcon("doi.org")).toBe(one);
    expect(await fetchSiteIcon("broken.example")).toBeNull();
    expect(await fetchSiteIcon("broken.example")).toBeNull();
    expect(invoke.mock.calls.map(([, args]) => args.url)).toEqual([
      "https://doi.org/",
      "https://broken.example/",
    ]);
  });
});
