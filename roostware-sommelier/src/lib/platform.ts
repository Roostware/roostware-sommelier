// Platform helpers. The app runs with ssr disabled, so `navigator`/`window` always exist,
// but the guards keep this module safe to import anywhere (e.g. during type checks).

/** True on macOS, where the native window buttons are kept (see tauri.macos.conf.json). */
export const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);

/** True inside the Tauri webview, false in a plain browser (e.g. `vite dev` in a tab). */
export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
