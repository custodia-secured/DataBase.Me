// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The clipboard (M7): the webview's own API, which works under the app's policy in a secure
 * context. Returns whether the text got there; a caller that gets `false` shows the text to
 * copy by hand (the diagnostic report does).
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
