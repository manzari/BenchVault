import { notifications } from "@mantine/notifications";

/// Wire-format error returned by every Rust command. Mirrors `service::AppError`.
/// Tauri rejects an `invoke()` promise with whatever the command returned as `Err(_)`;
/// for our commands that's a `{ code, message }` object, but if the rejection comes
/// from somewhere else (network, plugin, plain `Error`) we coerce it to the same shape.
export interface AppError {
  code: string;
  message: string;
}

const KNOWN_CODES = new Set([
  "io",
  "db",
  "json",
  "not_found",
  "conflict",
  "validation",
  "parser",
  "other",
  "unknown",
]);

/** Coerce anything thrown from `invoke()` (or elsewhere) into an `AppError`. */
export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object") {
    const anyE = e as Record<string, unknown>;
    if (typeof anyE.code === "string" && typeof anyE.message === "string") {
      return {
        code: KNOWN_CODES.has(anyE.code) ? anyE.code : "other",
        message: anyE.message,
      };
    }
    if (e instanceof Error) {
      return { code: "unknown", message: e.message };
    }
  }
  if (typeof e === "string") {
    return { code: "unknown", message: e };
  }
  return { code: "unknown", message: String(e) };
}

/** Show a red toast for an error and return the parsed `AppError` for further handling. */
export function notifyError(title: string, e: unknown): AppError {
  const err = toAppError(e);
  notifications.show({ color: "red", title, message: err.message });
  return err;
}

/** Success/failure result of {@link withErrorToast}. We can't use `null` as the
 *  failure sentinel: `void` commands (move, delete, …) resolve to `null` on
 *  *success*, which is indistinguishable from a `null`-on-failure marker — that
 *  collision silently skipped post-mutation refreshes. The explicit `ok` flag
 *  works regardless of what `T` is. */
export type ToastResult<T> = { ok: true; value: T } | { ok: false };

/** Wrap a promise-returning function so any rejection becomes a toast. Check
 *  `.ok` to branch on success; `.value` holds the resolved value when `ok`. */
export async function withErrorToast<T>(
  title: string,
  fn: () => Promise<T>,
): Promise<ToastResult<T>> {
  try {
    return { ok: true, value: await fn() };
  } catch (e) {
    notifyError(title, e);
    return { ok: false };
  }
}
