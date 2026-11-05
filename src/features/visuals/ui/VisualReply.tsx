import { createContext, type ReactNode, useContext } from "react";

import type { Block } from "../../sessions/model/session";
import {
  placedVisualIds,
  splitVisualEmbeds,
  type VisualDocument,
} from "../model/visual";
import { VisualView } from "./VisualView";

/** The visuals a conversation holds, so a reply can place them by id. */
export const VisualsContext = createContext<
  ReadonlyMap<string, VisualDocument>
>(new Map());

/**
 * A reply with each `![caption](visual:id)` line drawn as that visual.
 * `renderText` draws the prose between; `last` marks the part that streams.
 */
export function VisualReply({
  text,
  renderText,
}: {
  text: string;
  renderText: (text: string, last: boolean) => ReactNode;
}) {
  const visuals = useContext(VisualsContext);
  const parts = visuals.size ? splitVisualEmbeds(text) : null;
  if (!parts || !parts.some((part) => part.kind === "visual"))
    return renderText(text, true);
  return parts.map((part, index) => {
    if (part.kind === "text")
      return (
        <div key={index} className="min-w-0">
          {renderText(part.text, index === parts.length - 1)}
        </div>
      );
    const document = visuals.get(part.id);
    return document ? <VisualView key={index} document={document} /> : null;
  });
}

/** Visuals a turn published but its replies never placed, after the turn. */
export function UnplacedVisuals({ turn }: { turn: readonly Block[] }) {
  const placed = placedVisualIds(turn);
  const visuals = turn
    .flatMap((block) => block.visuals ?? [])
    .filter((document) => !placed.has(document.id));
  if (!visuals.length) return null;
  return (
    <div data-visual-results className="flex flex-col px-4 pb-1">
      {visuals.map((document) => (
        <VisualView key={document.id} document={document} />
      ))}
    </div>
  );
}
