// The languages a problem can be solved in. Slugs are LeetCode's `langSlug`s, which is what
// the catalog's starter code (and later, submissions) use.

import type { Draft } from "$lib/db";
import { LANGUAGE_ICONS, type LanguageIcon } from "$lib/languageIcons";

interface LanguageInfo {
  /** Name shown in the language picker, as LeetCode writes it. */
  label: string;
  /** Monaco's id for syntax highlighting. */
  monaco: string;
}

const LANGUAGES: Record<string, LanguageInfo> = {
  cpp: { label: "C++", monaco: "cpp" },
  java: { label: "Java", monaco: "java" },
  python: { label: "Python", monaco: "python" },
  python3: { label: "Python3", monaco: "python" },
  c: { label: "C", monaco: "c" },
  csharp: { label: "C#", monaco: "csharp" },
  javascript: { label: "JavaScript", monaco: "javascript" },
  typescript: { label: "TypeScript", monaco: "typescript" },
  php: { label: "PHP", monaco: "php" },
  swift: { label: "Swift", monaco: "swift" },
  kotlin: { label: "Kotlin", monaco: "kotlin" },
  dart: { label: "Dart", monaco: "dart" },
  golang: { label: "Go", monaco: "go" },
  ruby: { label: "Ruby", monaco: "ruby" },
  scala: { label: "Scala", monaco: "scala" },
  rust: { label: "Rust", monaco: "rust" },
  racket: { label: "Racket", monaco: "scheme" }, // no Racket grammar; Scheme is the closest
  erlang: { label: "Erlang", monaco: "plaintext" }, // Monaco has no Erlang grammar
  elixir: { label: "Elixir", monaco: "elixir" },
};

/** LeetCode's own ordering of its language picker. */
const ORDER = Object.keys(LANGUAGES);

export function languageLabel(slug: string): string {
  return LANGUAGES[slug]?.label ?? slug;
}

/** The language's icon for the picker, or undefined if it has none. */
export function languageIcon(slug: string): LanguageIcon | undefined {
  return LANGUAGE_ICONS[slug];
}

export function monacoLanguage(slug: string): string {
  return LANGUAGES[slug]?.monaco ?? "plaintext";
}

/** Sorts language slugs into LeetCode's order; any unknown ones go last, alphabetically. */
export function sortLanguages(slugs: string[]): string[] {
  const rank = (slug: string) => {
    const i = ORDER.indexOf(slug);
    return i === -1 ? ORDER.length : i;
  };
  return [...slugs].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}

/** The language to open: the user's last choice if this problem has it, else Python3, else the first. */
export function defaultLanguage(available: string[], preferred: string | null): string | null {
  if (preferred && available.includes(preferred)) return preferred;
  if (available.includes("python3")) return "python3";
  return available[0] ?? null;
}

/**
 * The language to open a problem in. If the user has unsubmitted code for it, that wins, so
 * their work is what they see: their usual language if it has a draft, else the most recently
 * edited draft. Otherwise it's the usual choice (see {@link defaultLanguage}).
 */
export function initialLanguage(
  available: string[],
  drafts: Pick<Draft, "lang" | "updatedAt">[],
  preferred: string | null,
): string | null {
  const drafted = drafts.filter((d) => available.includes(d.lang));
  if (drafted.length > 0) {
    if (preferred && drafted.some((d) => d.lang === preferred)) return preferred;
    return [...drafted].sort((a, b) => b.updatedAt - a.updatedAt)[0].lang;
  }
  return defaultLanguage(available, preferred);
}
