// What the dashboard's activity graph and daily streak are built from: the times the user made
// submissions, grouped by calendar day in their local time zone.

const MS_PER_DAY = 86_400_000;

/**
 * A number for a local calendar day: consecutive days differ by exactly 1, whatever the time
 * zone or daylight saving does to the length of the day. Only the date part of `date` counts.
 */
export function dayNumber(date: Date): number {
  return Math.floor(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / MS_PER_DAY);
}

/** The local calendar date as YYYY-MM-DD: what the backend keys each day's recommendations by. */
export function localDateKey(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/**
 * The rating's history as one value per local day for the graph: the starting rating (the first
 * entry, as the backend sends it), then the rating at the end of each day it changed.
 */
export function dailyRatings(history: { at: number; rating: number }[]): number[] {
  const [start, ...changes] = history;
  if (!start) return [];
  const values = [start.rating];
  let lastDay: number | null = null;
  for (const point of changes) {
    const day = dayNumber(new Date(point.at));
    if (day === lastDay) values[values.length - 1] = point.rating;
    else values.push(point.rating);
    lastDay = day;
  }
  return values;
}

/** How many of the submission times (epoch milliseconds) fall on each local day, by {@link dayNumber}. */
export function countByDay(times: number[]): Map<number, number> {
  const counts = new Map<number, number>();
  for (const time of times) {
    const day = dayNumber(new Date(time));
    counts.set(day, (counts.get(day) ?? 0) + 1);
  }
  return counts;
}

/**
 * Consecutive days with a submission, counting back from `today`. Today needn't have one yet:
 * the streak is only broken once a whole day has passed without, so a streak that ran up to
 * yesterday still counts until the day is over.
 */
export function currentStreak(counts: Map<number, number>, today: number): number {
  let day = counts.has(today) ? today : today - 1;
  let streak = 0;
  while (counts.has(day)) {
    streak++;
    day--;
  }
  return streak;
}
