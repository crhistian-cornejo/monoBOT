import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { Trash2 } from "../../../shared/ui/icons";
import { Switch } from "../../../shared/ui/Switch";
import { loadAutoImages, saveAutoImages, type MailAccount } from "../model/mail";
import { mailApi } from "../model/mailApi";

type Provider = "gmail" | "icloud" | "imap";

const PROVIDERS: { id: Provider; label: string }[] = [
  { id: "gmail", label: "Gmail" },
  { id: "icloud", label: "iCloud" },
  { id: "imap", label: "Other (IMAP)" },
];

/** How to make the app password each provider asks for. */
const STEPS: Record<Provider, { text: string; link?: { label: string; url: string } }> = {
  gmail: {
    text: "Gmail needs 2-Step Verification on, then an app password: create one named MonoBOT and paste its 16 letters here.",
    link: { label: "Create a Gmail app password", url: "https://myaccount.google.com/apppasswords" },
  },
  icloud: {
    text: "Sign in to your Apple Account, open Sign-In and Security, then App-Specific Passwords, and create one for MonoBOT.",
    link: { label: "Open your Apple Account", url: "https://account.apple.com/account/manage" },
  },
  imap: {
    text: "Use your mail server's IMAP address (port 993, TLS) and the password or app password your provider gives for mail apps.",
  },
};

const FIELD =
  "h-9 w-full rounded-md border border-content/10 bg-background-base px-2.5 text-[13px] outline-none focus:border-content/25";
const LABEL = "text-[12px] text-content/70";

/** Connect a mail account with an app password, or remove one. */
export function MailAccounts({
  accounts,
  onChanged,
}: {
  accounts: MailAccount[];
  onChanged: () => void;
}) {
  const [provider, setProvider] = useState<Provider>("gmail");
  const [address, setAddress] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [autoImages, setAutoImages] = useState(loadAutoImages);
  const steps = STEPS[provider];

  const connect = async () => {
    setBusy(true);
    setError(undefined);
    try {
      await mailApi.addAccount({
        address,
        password,
        name,
        provider,
        ...(provider === "imap" ? { host } : {}),
      });
      setAddress("");
      setPassword("");
      setName("");
      setHost("");
      onChanged();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      {accounts.length ? (
        <ul className="flex flex-col gap-1">
          {accounts.map((account) => (
            <li
              key={account.id}
              className="flex items-center gap-2 rounded-md border border-content/10 px-3 py-2 text-[13px]"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate">{account.name}</span>
                <span className="block truncate text-[12px] text-content/50">
                  {account.address}
                </span>
              </span>
              <button
                type="button"
                title="Remove this account from MonoBOT"
                aria-label={`Remove ${account.address}`}
                onClick={() =>
                  void mailApi.removeAccount(account.id).then(onChanged)
                }
                className="grid size-7 place-items-center rounded-md text-content/50 hover:bg-content/8 hover:text-content"
              >
                <Trash2 className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      ) : null}

      {accounts.length ? (
        <div className="flex items-start gap-3 rounded-md border border-content/10 px-3 py-2">
          <span className="min-w-0 flex-1">
            <span className="block text-[13px]">Load pictures automatically</span>
            <span className="block text-[12px] leading-relaxed text-content/55">
              MonoBOT fetches them for the message, with no cookies. A sender
              can still tell you opened it. Mail held back as suspicious never
              loads them on its own.
            </span>
          </span>
          <Switch
            label="Load pictures automatically"
            on={autoImages}
            onChange={(on) => {
              saveAutoImages(on);
              setAutoImages(on);
            }}
          />
        </div>
      ) : null}

      <form
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy) void connect();
        }}
      >
        <div role="radiogroup" aria-label="Mail provider" className="flex gap-1">
          {PROVIDERS.map((option) => (
            <button
              key={option.id}
              type="button"
              role="radio"
              aria-checked={provider === option.id}
              onClick={() => setProvider(option.id)}
              className={`rounded-md px-3 py-1.5 text-[12px] ${
                provider === option.id
                  ? "bg-selection text-content"
                  : "text-content/60 hover:bg-content/8"
              }`}
            >
              {option.label}
            </button>
          ))}
        </div>
        <p className="text-[12px] leading-relaxed text-content/60">
          {steps.text}{" "}
          {steps.link ? (
            <button
              type="button"
              onClick={() => void openUrl(steps.link!.url)}
              className="text-accent hover:underline"
            >
              {steps.link.label}
            </button>
          ) : null}
        </p>
        <label className="flex flex-col gap-1">
          <span className={LABEL}>Email address</span>
          <input
            className={FIELD}
            type="email"
            autoComplete="off"
            spellCheck={false}
            value={address}
            onChange={(event) => setAddress(event.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className={LABEL}>App password</span>
          <input
            className={FIELD}
            type="password"
            autoComplete="off"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        {provider === "imap" ? (
          <label className="flex flex-col gap-1">
            <span className={LABEL}>IMAP server</span>
            <input
              className={FIELD}
              placeholder="imap.example.com"
              spellCheck={false}
              value={host}
              onChange={(event) => setHost(event.target.value)}
            />
          </label>
        ) : null}
        <label className="flex flex-col gap-1">
          <span className={LABEL}>Name to show (optional)</span>
          <input
            className={FIELD}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <p className="text-[11px] text-content/45">
          The password stays in your system's keychain. MonoBOT reads your mail
          and sends only the replies you send yourself; it never deletes mail.
        </p>
        {error ? (
          <p role="alert" className="text-xs text-red-400">
            {error}
          </p>
        ) : null}
        <div>
          <button
            type="submit"
            disabled={busy || !address.trim() || !password.trim()}
            className="primary-action rounded-md px-3 py-1.5 text-[12px] disabled:cursor-default"
          >
            {busy ? "Connecting…" : "Connect account"}
          </button>
        </div>
      </form>
    </div>
  );
}
