import type { Element, Root } from "hast";
import { expect, it } from "vitest";
import {
  CITATION_GROUP_PROPERTY,
  groupedCitations,
  rehypeCitationGroups,
} from "./citationGroups";

const link = (n: number, href: string, title?: string): Element => ({
  type: "element",
  tagName: "a",
  properties: { href, ...(title ? { title } : {}) },
  children: [{ type: "text", value: String(n) }],
});

const paragraph = (children: Element["children"]): Root => ({
  type: "root",
  children: [{ type: "element", tagName: "p", properties: {}, children }],
});

it("folds side-by-side citations into the first chip", () => {
  const tree = paragraph([
    { type: "text", value: "Claim " },
    link(2, "https://web.dev/a", "Perf | Use transforms"),
    { type: "text", value: " " },
    link(3, "https://developer.mozilla.org/b", "MDN"),
    { type: "text", value: ". Next" },
  ]);
  rehypeCitationGroups()(tree);
  const p = tree.children[0] as Element;
  expect(p.children).toHaveLength(3);
  const chip = p.children[1] as Element;
  expect(groupedCitations(chip.properties[CITATION_GROUP_PROPERTY])).toEqual([
    { href: "https://developer.mozilla.org/b", title: "MDN" },
  ]);
});

it("leaves citations apart when words sit between them", () => {
  const tree = paragraph([
    link(1, "https://a.dev/x"),
    { type: "text", value: " and then " },
    link(2, "https://b.dev/y"),
  ]);
  rehypeCitationGroups()(tree);
  const p = tree.children[0] as Element;
  expect(p.children).toHaveLength(3);
  expect(
    (p.children[0] as Element).properties[CITATION_GROUP_PROPERTY],
  ).toBeUndefined();
});

it("drops a repeat of the same source", () => {
  const tree = paragraph([
    link(1, "https://a.dev/x"),
    link(1, "https://a.dev/x"),
  ]);
  rehypeCitationGroups()(tree);
  const p = tree.children[0] as Element;
  expect(p.children).toHaveLength(1);
  expect(groupedCitations("not json")).toEqual([]);
});
