// How much a reply (or a whole conversation) used, for the small line under the coach's answers:
// tokens as the provider counted them, and, if the user has said what their model costs, dollars.

/** 1,204 becomes "1.2k"; small numbers stay as they are. */
export function compactTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 100_000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  return `${Math.round(n / 1000)}k`;
}

/**
 * What `inputTokens` in and `outputTokens` out cost at the user's prices (US dollars per million
 * tokens), as "$0.03", or null when they haven't given both prices.
 */
export function costText(
  inputTokens: number,
  outputTokens: number,
  priceInput: number | null,
  priceOutput: number | null,
): string | null {
  if (priceInput === null || priceOutput === null) return null;
  const dollars = (inputTokens * priceInput + outputTokens * priceOutput) / 1_000_000;
  if (dollars <= 0) return "$0.00";
  if (dollars < 0.01) return "<$0.01";
  return `$${dollars < 1 ? dollars.toFixed(2) : dollars.toFixed(dollars < 10 ? 2 : 1)}`;
}

/** "1.2k in · 82 out · $0.03": the line under a reply. */
export function usageLine(
  inputTokens: number,
  outputTokens: number,
  priceInput: number | null,
  priceOutput: number | null,
): string {
  const cost = costText(inputTokens, outputTokens, priceInput, priceOutput);
  const parts = [`${compactTokens(inputTokens)} in`, `${compactTokens(outputTokens)} out`];
  if (cost) parts.push(cost);
  return parts.join(" · ");
}
