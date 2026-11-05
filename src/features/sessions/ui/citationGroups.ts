import type { Element, ElementContent, Root } from "hast";
import { citationNumber, isCitationHref } from "../model/citations";

/** A citation that rides on the chip in front of it. */
export type GroupedCitation = { href: string; title?: string };

export const CITATION_GROUP_PROPERTY = "dataCitationGroup";

function text(node: ElementContent): string {
  if (node.type === "text") return node.value;
  if (node.type === "element") return node.children.map(text).join("");
  return "";
}

function citation(node: ElementContent): GroupedCitation | null {
  if (node.type !== "element" || node.tagName !== "a") return null;
  const href = node.properties?.href;
  if (typeof href !== "string" || !isCitationHref(href)) return null;
  if (citationNumber(text(node)) === null) return null;
  const title = node.properties?.title;
  return { href, ...(typeof title === "string" ? { title } : {}) };
}

/**
 * Sources cited side by side, `[2](…)[3](…)`, back one claim: they become one
 * chip, `web.dev +1`, whose preview lists them all. The first link stays and
 * carries the others in a property; the ones after it and the space between
 * them go.
 */
export function rehypeCitationGroups() {
  return (tree: Root) => {
    const visit = (parent: Root | Element) => {
      const next: Root["children"] = [];
      const children: Root["children"] = parent.children;
      for (let index = 0; index < children.length; index += 1) {
        const original = children[index];
        const node = original as ElementContent;
        const lead = citation(node);
        if (!lead) {
          if (node.type === "element") visit(node);
          next.push(original);
          continue;
        }
        const rest: GroupedCitation[] = [];
        let end = index;
        for (let ahead = index + 1; ahead < children.length; ahead += 1) {
          const candidate = children[ahead] as ElementContent;
          const cited = citation(candidate);
          if (cited) {
            if (
              cited.href !== lead.href &&
              !rest.some((entry) => entry.href === cited.href)
            )
              rest.push(cited);
            end = ahead;
            continue;
          }
          if (text(candidate).trim() || candidate.type === "comment") break;
          if (candidate.type === "element" && candidate.tagName === "br") break;
        }
        if (end > index && rest.length) {
          const element = node as Element;
          element.properties = {
            ...element.properties,
            [CITATION_GROUP_PROPERTY]: JSON.stringify(rest),
          };
        }
        next.push(original);
        index = end;
      }
      parent.children = next as Element["children"];
    };
    visit(tree);
  };
}

/** The citations a chip carries besides its own, from its rendered prop. */
export function groupedCitations(value: unknown): GroupedCitation[] {
  if (typeof value !== "string") return [];
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed)
      ? parsed.filter(
          (entry): entry is GroupedCitation =>
            !!entry &&
            typeof entry === "object" &&
            typeof (entry as GroupedCitation).href === "string" &&
            isCitationHref((entry as GroupedCitation).href),
        )
      : [];
  } catch {
    return [];
  }
}
