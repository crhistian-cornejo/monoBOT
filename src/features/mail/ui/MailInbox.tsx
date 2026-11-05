import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Modal } from "../../../shared/ui/Modal";
import { ScrollStrip } from "../../../shared/ui/ScrollStrip";
import { Mail, RefreshCw, Search, Settings, Star } from "../../../shared/ui/icons";
import {
  MAIL_VIEWS,
  applyAction,
  byDay,
  rowTime,
  unreadCount,
  visibleMessages,
  type MailAccount,
  type MailAction,
  type MailOverview,
  type MailView,
} from "../model/mail";
import { mailApi, onMailChanged } from "../model/mailApi";
import { MailAccounts } from "./MailAccounts";
import { MailArt } from "./MailArt";
import { MailReader } from "./MailReader";

/** The user's mail, beside GitHub in the Inbox: read it, sort it, reply with AI. */
export function MailInbox() {
  const [accounts, setAccounts] = useState<MailAccount[]>();
  const [overview, setOverview] = useState<MailOverview>();
  const [view, setView] = useState<MailView>("inbox");
  const [accountId, setAccountId] = useState<string>();
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [replyRequest, setReplyRequest] = useState(0);
  const latest = useRef(0);

  const read = useCallback(async (force: boolean) => {
    const ticket = ++latest.current;
    setLoading(true);
    setError(undefined);
    try {
      const next = await mailApi.overview(force);
      // The newest reading wins over one that came back late.
      if (ticket === latest.current) setOverview(next);
    } catch (failure) {
      if (ticket === latest.current) setError(String(failure));
    } finally {
      if (ticket === latest.current) setLoading(false);
    }
  }, []);

  const loadAccounts = useCallback(async () => {
    const list = await mailApi.accounts().catch(() => []);
    setAccounts(list);
    if (!list.length) return setOverview(undefined);
    // The last reading paints at once (memory or the snapshot on disk); the
    // fresh one replaces it when it lands.
    const last = await mailApi.last().catch(() => null);
    if (last) setOverview((current) => current ?? last);
    void read(false);
  }, [read]);

  useEffect(() => {
    void loadAccounts();
    const stop = onMailChanged(() => void read(true));
    return () => void stop.then((unlisten) => unlisten());
  }, [loadAccounts, read]);

  const messages = overview?.messages ?? [];
  const shown = useMemo(
    () => visibleMessages(messages, view, { accountId, query }),
    [messages, view, accountId, query],
  );
  const selected = messages.find((message) => message.id === selectedId);

  // Read ahead what is likely opened next: the top of the list, and the
  // messages beside the open one. Opening them then comes from memory.
  const shownIds = shown.map((message) => message.id).join(" ");
  useEffect(() => {
    const ids = shownIds ? shownIds.split(" ") : [];
    const at = selectedId ? ids.indexOf(selectedId) : -1;
    const next = at >= 0 ? [ids[at + 1], ids[at - 1]] : ids.slice(0, 3);
    const wanted = next.filter((id): id is string => !!id);
    if (wanted.length) void mailApi.prefetch(wanted).catch(() => undefined);
  }, [shownIds, selectedId]);
  const hoverTimer = useRef<number | undefined>(undefined);
  const prefetchSoon = (id: string) => {
    window.clearTimeout(hoverTimer.current);
    hoverTimer.current = window.setTimeout(
      () => void mailApi.prefetch([id]).catch(() => undefined),
      120,
    );
  };

  const act = useCallback(
    (id: string, action: MailAction) => {
      setOverview((current) =>
        current ? { ...current, messages: applyAction(current.messages, id, action) } : current,
      );
      void mailApi.act(id, action).catch((failure) => {
        setError(String(failure));
        void read(true);
      });
    },
    [read],
  );

  const open = (id: string) => {
    setSelectedId(id);
    const message = messages.find((entry) => entry.id === id);
    if (message?.unread) act(id, "read");
  };

  // J/K move, E archives, S stars, R replies: outside text fields only.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable=true]")) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const index = shown.findIndex((message) => message.id === selectedId);
      if (event.key === "j" || event.key === "k") {
        const next = shown[Math.min(shown.length - 1, Math.max(0, index + (event.key === "j" ? 1 : -1)))];
        if (next) open(next.id);
      } else if (event.key === "e" && selected && selected.folder !== "archive") {
        act(selected.id, "archive");
      } else if (event.key === "s" && selected) {
        act(selected.id, selected.starred ? "unstar" : "star");
      } else if (event.key === "r" && selected) {
        setReplyRequest((count) => count + 1);
      } else return;
      event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (accounts === undefined)
    return <p className="p-5 text-[13px] text-content/50">Loading mail…</p>;

  if (!accounts.length)
    return (
      <div className="flex min-h-0 min-w-0 flex-1">
      <div className="min-h-0 min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-md px-5 py-8">
          <h2 className="flex items-center gap-2 text-[15px] font-medium">
            <Mail className="size-4 text-accent" /> Your email in the Inbox
          </h2>
          <p className="mb-5 mt-1 text-[13px] text-content/60">
            Read your mail beside GitHub, sort it, and answer with a reply AI
            drafts for you to review. Connect an account with an app password.
          </p>
          <MailAccounts accounts={accounts} onChanged={() => void loadAccounts()} />
        </div>
      </div>
      <div className="hidden min-h-0 w-[42%] shrink-0 border-l border-stroke md:block">
        <MailArt />
      </div>
      </div>
    );

  const groups = byDay(shown);

  return (
    <div className="flex min-h-0 min-w-0 flex-1">
      <div className="flex min-h-0 w-[340px] shrink-0 flex-col border-r border-stroke">
        <div className="shrink-0 border-b border-stroke">
        <ScrollStrip activeKey={view} label="folders" className="gap-1 px-2 py-1.5">
          {MAIL_VIEWS.map((option) => {
            const count = unreadCount(messages, option.view);
            return (
              <button
                key={option.view}
                type="button"
                data-active={view === option.view ? "" : undefined}
                onClick={() => {
                  setView(option.view);
                  setSelectedId(undefined);
                }}
                className={`flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-[12px] ${
                  view === option.view
                    ? "bg-selection text-content"
                    : "text-content/55 hover:bg-content/5 hover:text-content"
                }`}
              >
                {option.label}
                {count && option.view !== "held" ? (
                  <span className="text-[10px] tabular-nums text-accent">{count}</span>
                ) : null}
              </button>
            );
          })}
        </ScrollStrip>
        </div>
        <div className="flex h-9 shrink-0 items-center gap-1 border-b border-stroke px-2">
          <div className="relative flex h-7 min-w-0 flex-1 items-center">
            <Search className="pointer-events-none absolute left-2 size-3 opacity-50" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search mail"
              aria-label="Search mail"
              spellCheck={false}
              className="h-7 w-full rounded-md bg-transparent pl-7 pr-2 text-[12px] outline-none placeholder:text-content/40"
            />
          </div>
          {accounts.length > 1 ? (
            <select
              aria-label="Account"
              value={accountId ?? ""}
              onChange={(event) => setAccountId(event.target.value || undefined)}
              className="h-7 max-w-28 rounded-md bg-transparent text-[12px] text-content/70 outline-none"
            >
              <option value="">All accounts</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>
                  {account.name}
                </option>
              ))}
            </select>
          ) : null}
          <button
            type="button"
            title="Check for new mail"
            aria-label="Refresh"
            onClick={() => void read(true)}
            className="grid size-7 place-items-center rounded-md text-content/55 hover:bg-content/8 hover:text-content"
          >
            <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />
          </button>
          <button
            type="button"
            title="Mail accounts"
            aria-label="Mail accounts"
            onClick={() => setSettingsOpen(true)}
            className="grid size-7 place-items-center rounded-md text-content/55 hover:bg-content/8 hover:text-content"
          >
            <Settings className="size-3.5" />
          </button>
        </div>
        {[...(overview?.problems ?? []), ...(error ? [error] : [])].map((problem) => (
          <p key={problem} role="alert" className="border-b border-stroke px-3 py-2 text-[12px] text-red-400">
            {problem}
          </p>
        ))}
        <div className="min-h-0 flex-1 overflow-y-auto">
          {!overview && loading ? (
            <p className="p-4 text-[12px] text-content/50">Reading your mail…</p>
          ) : !shown.length ? (
            <p className="p-4 text-[12px] text-content/50">
              {query ? "Nothing matches." : "Nothing here."}
            </p>
          ) : (
            groups.map((group) => (
              <section key={group.day}>
                <h3 className="sticky top-0 bg-background-base px-3 pb-1 pt-2 text-[11px] text-content/45">
                  {group.day}
                </h3>
                {group.messages.map((message) => (
                  <button
                    key={message.id}
                    type="button"
                    onClick={() => open(message.id)}
                    onMouseEnter={() => prefetchSoon(message.id)}
                    aria-current={message.id === selectedId ? "true" : undefined}
                    className={`flex w-full flex-col gap-0.5 border-b border-content/5 px-3 py-2 text-left ${
                      message.id === selectedId ? "bg-selection" : "hover:bg-content/4"
                    }`}
                  >
                    <span className="flex items-center gap-1.5 text-[12px]">
                      {message.unread ? (
                        <span aria-label="Unread" className="size-1.5 shrink-0 rounded-full bg-accent" />
                      ) : null}
                      <span className={`min-w-0 flex-1 truncate ${message.unread ? "font-medium text-content" : "text-content/75"}`}>
                        {message.from}
                      </span>
                      {message.starred ? <Star className="size-3 shrink-0 text-accent" /> : null}
                      <span className="shrink-0 text-[11px] text-content/40">{rowTime(message.at)}</span>
                    </span>
                    <span className={`truncate text-[12px] ${message.unread ? "text-content" : "text-content/70"}`}>
                      {message.subject || "(no subject)"}
                    </span>
                    <span className="truncate text-[11px] text-content/45">{message.preview}</span>
                  </button>
                ))}
              </section>
            ))
          )}
          {view === "archive" && shown.length ? (
            <button
              type="button"
              onClick={() =>
                void mailApi.archiveMore().then(setOverview).catch((failure) => setError(String(failure)))
              }
              className="m-2 rounded-md px-3 py-1.5 text-[12px] text-content/60 hover:bg-content/8"
            >
              Load more
            </button>
          ) : null}
        </div>
      </div>

      {selected ? (
        <MailReader
          message={selected}
          account={accounts.find((account) => account.id === selected.accountId)}
          replyRequest={replyRequest}
          onBack={() => setSelectedId(undefined)}
          onAct={(action) => act(selected.id, action)}
          onInteresting={(on) =>
            void mailApi
              .interesting(selected.address, on)
              .then(() => read(true))
              .catch((failure) => setError(String(failure)))
          }
        />
      ) : (
        <div className="min-h-0 min-w-0 flex-1">
          <MailArt>
            <p className="text-[13px] font-medium text-content">
              {unreadCount(messages, "inbox")
                ? `${unreadCount(messages, "inbox")} unread in your inbox`
                : "You're up to date"}
            </p>
            <p className="mt-1 text-[12px] text-content/60">
              Pick a message · J/K to move, E archive, S star, R reply
            </p>
          </MailArt>
        </div>
      )}

      {settingsOpen ? (
        <Modal title="Mail accounts" size="md" onClose={() => setSettingsOpen(false)}>
          <div className="px-5 py-4">
            <MailAccounts
              accounts={accounts}
              onChanged={() => {
                void loadAccounts();
              }}
            />
          </div>
        </Modal>
      ) : null}
    </div>
  );
}
