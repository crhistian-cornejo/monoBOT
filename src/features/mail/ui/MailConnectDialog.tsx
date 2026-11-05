import { useEffect, useState } from "react";
import { Modal } from "../../../shared/ui/Modal";
import type { MailAccount } from "../model/mail";
import { onMailConnectRequest, type MailConnectRequest } from "../model/mailAccess";
import { mailApi } from "../model/mailApi";
import { MailAccounts } from "./MailAccounts";

/**
 * The dialog a bot opens with `mail.connect`: the Email tab's account form,
 * its address filled in, so the owner types the app password here and never
 * in a chat. Mounted once, at the app's root.
 */
export function MailConnectDialog() {
  const [request, setRequest] = useState<MailConnectRequest>();
  const [accounts, setAccounts] = useState<MailAccount[]>([]);
  const load = () => void mailApi.accounts().then(setAccounts, () => setAccounts([]));

  useEffect(
    () =>
      onMailConnectRequest((next) => {
        setRequest(next);
        load();
      }),
    [],
  );

  if (!request) return null;
  return (
    <Modal
      title="Connect an email account"
      description={`${request.askedBy} wants to read ${request.address}.`}
      size="md"
      onClose={() => setRequest(undefined)}
    >
      <div className="px-5 py-4">
        <MailAccounts
          key={`${request.address}:${request.onlyFor ?? ""}`}
          accounts={accounts}
          request={request}
          onChanged={load}
          onConnected={() => setRequest(undefined)}
        />
      </div>
    </Modal>
  );
}
