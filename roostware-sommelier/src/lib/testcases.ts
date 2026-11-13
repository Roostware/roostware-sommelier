// Test cases for the Run button, in the shape LeetCode's own editor uses: a case is one value
// per argument of the function, and a problem's example cases are stored as one big text with a
// line per argument, case after case. The catalog has no signature data, so the argument
// names (and how many there are) come from the Python 3 starter code.

/** One test case: the text of each argument, in order. */
export type TestCase = string[];

export interface CaseLayout {
  /** Argument names, in order. */
  params: string[];
  /** The problem's own example cases. */
  examples: TestCase[];
}

/** Splits at commas that aren't inside brackets, so `a: List[int], b: Dict[str, int]` gives two parts. */
function splitTopLevel(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const ch of text) {
    if ("([{".includes(ch)) depth++;
    else if (")]}".includes(ch)) depth--;
    if (ch === "," && depth === 0) {
      parts.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  if (current.trim()) parts.push(current);
  return parts.map((p) => p.trim()).filter(Boolean);
}

/**
 * The argument names of the method LeetCode calls, read from the Python 3 starter code, or null
 * when it isn't a plain "call one method of Solution" problem (design problems, say).
 */
export function parameterNames(pythonStarter: string | undefined): string[] | null {
  if (!pythonStarter) return null;
  // Starters often carry a commented-out helper class (ListNode, TreeNode) before the real one.
  const code = pythonStarter
    .split("\n")
    .filter((line) => !line.trim().startsWith("#"))
    .join("\n");
  const solution = code.match(/^class\s+Solution\b/m);
  if (!solution) return null;
  const def = /def\s+\w+\s*\(/.exec(code.slice(solution.index));
  if (!def) return null;

  // The signature's parentheses, found by balancing (a type annotation could contain some).
  const start = (solution.index ?? 0) + def.index + def[0].length;
  let depth = 1;
  let end = start;
  while (end < code.length && depth > 0) {
    if (code[end] === "(") depth++;
    else if (code[end] === ")") depth--;
    end++;
  }
  if (depth !== 0) return null;

  const names = splitTopLevel(code.slice(start, end - 1))
    .map((arg) => arg.split(":")[0].split("=")[0].trim())
    .filter((name) => name && name !== "self" && !name.startsWith("*"));
  return names.length > 0 ? names : null;
}

/** Cuts raw example input (a line per argument, case after case) into cases, or null if the lines don't divide evenly. */
export function splitCases(raw: string, paramCount: number): TestCase[] | null {
  if (paramCount < 1) return null;
  const lines = raw.replace(/\r\n/g, "\n").replace(/\n+$/, "").split("\n");
  if (lines.length === 0 || lines.length % paramCount !== 0 || (lines.length === 1 && lines[0] === "")) return null;
  const cases: TestCase[] = [];
  for (let i = 0; i < lines.length; i += paramCount) cases.push(lines.slice(i, i + paramCount));
  return cases;
}

/** The text LeetCode wants for a list of cases: a line per argument, case after case. */
export function joinCases(cases: TestCase[]): string {
  return cases.map((c) => c.join("\n")).join("\n");
}

/**
 * How to lay out a problem's test cases: its argument names and its example cases, or null when
 * the input can't be split into arguments (a handful of problems with unusual input formats).
 */
export function layoutFor(exampleTestCases: string, pythonStarter: string | undefined): CaseLayout | null {
  // Design problems feed two lines per case: the method names (starting with the class's name),
  // then their arguments. Checked first because some of them (Shuffle an Array...) have a
  // `Solution` class that would otherwise be mistaken for a plain function.
  const design = splitCases(exampleTestCases, 2);
  if (design && design.every(([names, args]) => /^\["[A-Z]\w*"/.test(names) && /^\[(\[|\])/.test(args))) {
    return { params: ["operations", "arguments"], examples: design };
  }

  const names = parameterNames(pythonStarter);
  if (!names) return null;
  const examples = splitCases(exampleTestCases, names.length);
  return examples ? { params: names, examples } : null;
}

/** A layout that always exists: the real one, or one editable block of text holding the whole example input. */
export interface Layout extends CaseLayout {
  /** True when the arguments couldn't be told apart, so there is just one text box and cases can't be added. */
  freeform: boolean;
}

export function layoutOrFreeform(exampleTestCases: string, pythonStarter: string | undefined): Layout {
  const layout = layoutFor(exampleTestCases, pythonStarter);
  if (layout) return { ...layout, freeform: false };
  return { params: ["input"], examples: [[exampleTestCases.replace(/\r\n/g, "\n").replace(/\n+$/, "")]], freeform: true };
}

/**
 * Cuts a failing input reported by LeetCode into one case, if it has exactly one value per argument.
 * (It carries one line per argument, the same shape as an example.)
 */
export function caseFromInput(input: string, paramCount: number): TestCase | null {
  const cases = splitCases(input, paramCount);
  return cases && cases.length === 1 ? cases[0] : null;
}

/** True if any argument of any case is blank, which LeetCode would reject. */
export function hasBlank(cases: TestCase[]): boolean {
  return cases.some((c) => c.some((value) => value.trim() === ""));
}

export function sameCases(a: TestCase[], b: TestCase[]): boolean {
  return a.length === b.length && a.every((c, i) => c.length === b[i].length && c.every((v, j) => v === b[i][j]));
}
