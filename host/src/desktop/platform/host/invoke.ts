/**
 * The host runs without the desktop shell. Adapter code shared with the
 * former desktop app still names its commands; here they reject, exactly as
 * Tauri's `invoke` rejects when no window exists. Provider processes go
 * through the configured child backend instead (see `core/child.ts`).
 */
export type UnlistenFn = () => void;

export function invoke<T>(
  command: string,
  _args?: Record<string, unknown>,
): Promise<T> {
  return Promise.reject(
    new Error(`${command} is not available on monoBOT Host`),
  );
}

export function listen<T>(
  event: string,
  _handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  return Promise.reject(
    new Error(`${event} events are not available on monoBOT Host`),
  );
}
