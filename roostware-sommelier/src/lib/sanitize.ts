// Problem statements come from a third-party dataset as HTML, so they are sanitized before
// being rendered. Beyond stripping anything that can run code, this makes them fit the app:
// no inline colors (they'd be unreadable on the dark theme), no inline sizing except image
// widths, and links/images that can't leak or navigate the app away.

import DOMPurify from "dompurify";

DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (!(node instanceof Element)) return;

  // Inline styles carry colors, fonts and fixed sizes that fight the app's theme. Keep only an
  // image's pixel width (the CSS caps it to the panel and keeps the aspect ratio).
  const width = node instanceof HTMLElement && node.tagName === "IMG" ? node.style.width : "";
  node.removeAttribute("style");
  if (width && /^\d+(\.\d+)?px$/.test(width)) (node as HTMLElement).style.width = width;

  if (node.tagName === "IMG") {
    node.setAttribute("loading", "lazy");
    node.setAttribute("decoding", "async");
    node.setAttribute("referrerpolicy", "no-referrer");
  }
  if (node.tagName === "A") node.setAttribute("rel", "noopener noreferrer");
});

/** The statement HTML, made safe to render with `{@html}`. */
export function sanitizeStatement(html: string): string {
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true, svg: true },
    FORBID_TAGS: [
      "style", "form", "input", "button", "select", "textarea", "video", "audio", "source",
      "track", "iframe", "object", "embed", "link", "meta", "base",
    ],
    FORBID_ATTR: ["color", "bgcolor", "bordercolor", "face", "size", "target"],
    ALLOW_DATA_ATTR: false,
  });
}
