import { handleEmail } from "./email.ts";
import { handleRequest } from "./http.ts";
import { runRetention } from "./retention.ts";
import type { Env, InboundEmail, ScheduledEventLike } from "./types.ts";

export default {
  fetch(request: Request, env: Env): Promise<Response> {
    return handleRequest(request, env);
  },
  email(message: InboundEmail, env: Env): Promise<void> {
    return handleEmail(message, env);
  },
  async scheduled(_event: ScheduledEventLike, env: Env): Promise<void> {
    const deleted = await runRetention(env);
    console.log(`inbound: retention removed ${deleted} messages`);
  },
};
