import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "$lib/platform";

/**
 * Opens a link in the system browser. Only web and mail links are allowed, and relative
 * links (as problem statements use for other problems) are resolved against leetcode.com.
 * Returns false if the link was refused.
 */
export async function openExternal(href: string): Promise<boolean> {
  let url: URL;
  try {
    url = new URL(href, "https://leetcode.com");
  } catch {
    return false;
  }
  if (!["http:", "https:", "mailto:"].includes(url.protocol)) return false;
  if (isTauri) await openUrl(url.href);
  else window.open(url.href, "_blank", "noopener,noreferrer");
  return true;
}
