/**
 * Session and block fields owned by desktop features (notes, inbox, handoffs,
 * artifacts, visuals, Monos, orchestration). The host never creates them; it
 * keeps whatever a stored snapshot already has and passes it through, so only
 * their presence is typed here.
 */
type DesktopRecord = { readonly [key: string]: unknown };

export type HandoffComposerCard = DesktopRecord;
export type InboxComposerCard = DesktopRecord;
export type InboxAskContext = DesktopRecord;
export type NoteCardMeta = DesktopRecord;
export type NoteComposerCard = DesktopRecord;
export type LinkedWorkItemUpdateCard = DesktopRecord;
export type ArtifactCard = DesktopRecord;
export type VisualDocument = DesktopRecord;
export type MonoCard = DesktopRecord;

/** Settling a turn marks an unfinished orchestration plan invalid. */
export type OrchestrationProposal = DesktopRecord & {
  status?: string;
  error?: string;
};
