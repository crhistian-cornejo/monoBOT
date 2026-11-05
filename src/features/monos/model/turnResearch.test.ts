import { expect, it } from "vitest";
import type { Block } from "../../sessions/model/session";
import { turnResearch } from "./turnResearch";

const turn: Block[] = [
  { id: "u", role: "user", text: "Find a paper" },
  { id: "intro", role: "assistant", text: "Searching now." },
  {
    id: "s",
    role: "tool",
    text: "Find hydrology R",
    tool: {
      kind: "search",
      preview: { kind: "search", query: "hydrology R" },
      detail:
        'Web search results for query: "hydrology R"\n\nLinks: [{"title":"airGR teaching","url":"https://hal.science/a"},{"title":"Other","url":"https://example.com/b"}]\n\nMore text',
    },
  },
  {
    id: "f",
    role: "tool",
    text: "Read https://doi.org/10.1/x",
    tool: {
      kind: "WebFetch",
      preview: { kind: "read", url: "https://doi.org/10.1/x" },
    },
  },
  {
    id: "a",
    role: "assistant",
    text: 'The suite works [1](https://doi.org/10.1/x "The suite of GR models | It works."). Also [2](https://hal.science/a).',
  },
];

it("lists searches, then sources with the cited ones first", () => {
  const research = turnResearch(turn);
  expect(research.searches).toEqual([
    { id: "s", query: "hydrology R", results: 2 },
  ]);
  expect(research.sources).toEqual([
    {
      url: "https://doi.org/10.1/x",
      host: "doi.org",
      read: true,
      cited: 1,
      title: "The suite of GR models",
    },
    {
      url: "https://hal.science/a",
      host: "hal.science",
      read: false,
      cited: 2,
      title: "airGR teaching",
    },
  ]);
  expect(research.skipped).toBe(1);
  expect(research.answerId).toBe("a");
});

it("keeps a turn without web work empty", () => {
  expect(
    turnResearch([
      { id: "u", role: "user", text: "Hi" },
      { id: "a", role: "assistant", text: "Hello" },
    ]),
  ).toEqual({ searches: [], sources: [], skipped: 0, answerId: "a" });
});
