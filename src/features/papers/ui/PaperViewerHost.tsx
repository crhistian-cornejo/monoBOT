import { useEffect, useState } from "react";
import { Modal } from "../../../shared/ui/Modal";
import { OPEN_PAPER_EVENT, type OpenPaperRequest } from "../model/openPaper";
import { PaperReader } from "./PaperReader";

/**
 * Shows a paper when a citation to one is clicked anywhere in the app. Mounted
 * once; whoever has a paper to show sends `openPaper`.
 */
export function PaperViewerHost() {
  const [request, setRequest] = useState<OpenPaperRequest | null>(null);
  useEffect(() => {
    const open = (event: Event) =>
      setRequest((event as CustomEvent<OpenPaperRequest>).detail);
    window.addEventListener(OPEN_PAPER_EVENT, open);
    return () => window.removeEventListener(OPEN_PAPER_EVENT, open);
  }, []);
  if (!request) return null;
  return (
    <Modal
      title="Paper"
      size="lg"
      minimalHeader
      fitViewport
      className="h-[min(86vh,900px)]"
      onClose={() => setRequest(null)}
    >
      <PaperReader paperKey={request.key} passage={request.passage} />
    </Modal>
  );
}
