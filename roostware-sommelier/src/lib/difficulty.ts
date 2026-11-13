import type { Difficulty } from "$lib/db";

/** The dot/label color used for each difficulty across the app. */
export const difficultyColor: Record<Difficulty, string> = {
  Easy: "#3FAE7F",
  Medium: "#EBA337",
  Hard: "#FF5555",
};
