/**
 * Where the chosen model is remembered in the browser, per provider.
 *
 * Settings draws its model field from this and the chat's "use the default"
 * button writes it, so the two agree about which model is selected. The
 * agent itself is pointed at a model by `set_active_model`, which the
 * backend persists; this only decides what the field shows (#249).
 */

/** `edytlab.model.<provider>` in `localStorage`. */
export const MODEL_STORAGE_KEY_PREFIX = "edytlab.model.";

export function modelStorageKey(provider: string): string {
  return `${MODEL_STORAGE_KEY_PREFIX}${provider}`;
}
