<script lang="ts">
  import { onMount } from "svelte";
  import { openProblem } from "$lib/solve.svelte";

  // TODO: replace with the signed-in user's name once it's available.
  const userName = "EulerNumber";

  // TODO: replace with the user's real solved-problem counts by difficulty.
  const solvedByDifficulty = [
    { label: "Easy", count: 64, color: "#3FAE7F" },
    { label: "Medium", count: 48, color: "#EBA337" },
    { label: "Hard", count: 16, color: "#FF5555" },
  ];

  const problemsSolved = solvedByDifficulty.reduce((sum, d) => sum + d.count, 0);

  // TODO: replace with the user's real rating history (oldest first, last is current)
  // and the real change over the current week.
  const ratingHistory = [1540, 1555, 1548, 1572, 1590, 1585, 1610, 1618, 1642];
  const rating = ratingHistory[ratingHistory.length - 1];
  const weeklyChange = 24;

  const GRAPH_PAD = 8;
  let graphWidth = $state(0);
  let graphHeight = $state(0);

  let graphPoints = $derived.by(() => {
    if (graphWidth === 0 || graphHeight === 0) return [];
    const min = Math.min(...ratingHistory);
    const range = Math.max(...ratingHistory) - min || 1;
    const lastIndex = Math.max(ratingHistory.length - 1, 1);
    return ratingHistory.map((value, i) => ({
      x: GRAPH_PAD + (i / lastIndex) * (graphWidth - 2 * GRAPH_PAD),
      y: GRAPH_PAD + (1 - (value - min) / range) * (graphHeight - 2 * GRAPH_PAD),
    }));
  });
  let linePath = $derived(
    graphPoints.map((p, i) => `${i === 0 ? "M" : "L"}${p.x} ${p.y}`).join(" "),
  );
  let currentPoint = $derived(graphPoints[graphPoints.length - 1]);

  function percentOf(count: number) {
    return problemsSolved === 0 ? 0 : Math.round((count / problemsSolved) * 100);
  }

  function greetingFor(date: Date) {
    const hour = date.getHours();
    if (hour >= 5 && hour < 12) return "Good morning";
    if (hour >= 12 && hour < 17) return "Good afternoon";
    return "Good evening";
  }

  let now = $state(new Date());
  let greeting = $derived(greetingFor(now));

  onMount(() => {
    // Keep the greeting and the streak's "today" fresh if the app stays open
    // across a time-of-day or day boundary.
    const timer = setInterval(() => {
      now = new Date();
    }, 60_000);
    return () => clearInterval(timer);
  });

  const prefersReducedMotion =
    typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // Counts a number up from 0 when it first appears. The element is sized for the
  // final value up front so surrounding content doesn't shift while it counts.
  function countUp(node: HTMLElement, { value, delay = 0 }: { value: number; delay?: number }) {
    node.style.minWidth = `${String(value).length}ch`;
    if (prefersReducedMotion) {
      node.textContent = String(value);
      return;
    }

    const duration = 800;
    let frame = 0;
    node.textContent = "0";
    const startAt = performance.now() + delay;
    const tick = (time: number) => {
      const progress = Math.min(Math.max((time - startAt) / duration, 0), 1);
      node.textContent = String(Math.round(value * (1 - Math.pow(1 - progress, 3))));
      if (progress < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return { destroy: () => cancelAnimationFrame(frame) };
  }

  type Recommendation = {
    id: number;
    name: string;
    difficulty: "Easy" | "Medium" | "Hard";
    // "new" problems haven't been solved yet; "review" ones were selected for review.
    kind: "new" | "review";
    lastSolved: string | null; // ISO date (YYYY-MM-DD), only for "review"
  };

  // TODO: replace with the real recommendations.
  const recommendations: Recommendation[] = [
    { id: 1, name: "Two Sum", difficulty: "Easy", kind: "new", lastSolved: null },
    {
      id: 3,
      name: "Longest Substring Without Repeating Characters",
      difficulty: "Medium",
      kind: "review",
      lastSolved: "2026-10-21",
    },
    { id: 15, name: "3Sum", difficulty: "Medium", kind: "new", lastSolved: null },
    { id: 42, name: "Trapping Rain Water", difficulty: "Hard", kind: "review", lastSolved: "2026-09-30" },
    { id: 200, name: "Number of Islands", difficulty: "Medium", kind: "review", lastSolved: "2026-10-29" },
    { id: 20, name: "Valid Parentheses", difficulty: "Easy", kind: "new", lastSolved: null },
  ];

  // Listed new problems first, then review ones; within each group easiest first,
  // then by problem number.
  const KIND_ORDER = { new: 0, review: 1 };
  const DIFFICULTY_ORDER = { Easy: 0, Medium: 1, Hard: 2 };
  const sortedRecommendations = [...recommendations].sort(
    (a, b) =>
      KIND_ORDER[a.kind] - KIND_ORDER[b.kind] ||
      DIFFICULTY_ORDER[a.difficulty] - DIFFICULTY_ORDER[b.difficulty] ||
      a.id - b.id,
  );

  const difficultyColor: Record<string, string> = Object.fromEntries(
    solvedByDifficulty.map((d) => [d.label, d.color]),
  );

  function formatDate(iso: string) {
    const date = new Date(`${iso}T00:00:00`);
    return date.toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
    });
  }

  // Activity levels 0 (none) to 4 (most), lightest to brightest.
  const ACTIVITY_COLORS = ["#33373f", "#61562e", "#8b762a", "#ba9926", "#e9bc22"];
  const ACTIVITY_WEEKS = 53; // 52 full weeks plus the current (partial) one

  function activityLevel(count: number) {
    if (count === 0) return 0;
    if (count <= 2) return 1;
    if (count <= 4) return 2;
    if (count <= 6) return 3;
    return 4;
  }

  // TODO: replace with the user's real problems-solved count per day.
  // Placeholder: a stable pseudo-random count derived from the day number.
  function placeholderCount(dayNumber: number) {
    const hash = Math.imul(dayNumber, 2654435761) >>> 0;
    return [0, 0, 0, 1, 1, 2, 3, 4, 6, 8][(hash >>> 16) % 10];
  }

  // One entry per day, oldest first, starting on a Sunday so each column is a week
  // (Sunday at the top) and ending today.
  let activityDays = $derived.by(() => {
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const start = new Date(today);
    start.setDate(today.getDate() - today.getDay() - (ACTIVITY_WEEKS - 1) * 7);

    const days: { date: Date; count: number; level: number }[] = [];
    for (const d = new Date(start); d <= today; d.setDate(d.getDate() + 1)) {
      const date = new Date(d);
      const dayNumber = Math.floor(
        Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 86_400_000,
      );
      const count = placeholderCount(dayNumber);
      days.push({ date, count, level: activityLevel(count) });
    }
    return days;
  });

  // Month names above the column where each month begins.
  let monthLabels = $derived.by(() => {
    const monthName = (date: Date) => date.toLocaleDateString(undefined, { month: "short" });
    const labels: { col: number; text: string }[] = [];
    activityDays.forEach((day, i) => {
      if (day.date.getDate() === 1) labels.push({ col: Math.floor(i / 7), text: monthName(day.date) });
    });
    // Also name the month the grid starts in, unless the next label is too close.
    if (labels.length === 0 || labels[0].col >= 3) {
      labels.unshift({ col: 0, text: monthName(activityDays[0].date) });
    }
    return labels;
  });

  let totalSubmissions = $derived(activityDays.reduce((sum, d) => sum + d.count, 0));
  let activeDayCount = $derived(activityDays.filter((d) => d.count > 0).length);

  function formatLongDate(date: Date) {
    return date.toLocaleDateString(undefined, {
      weekday: "short",
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  }

  // Custom hover tooltip for the activity cells (one shared element, positioned
  // above the hovered cell in viewport coordinates).
  let hoveredCell = $state<{ day: { date: Date; count: number }; x: number; y: number } | null>(
    null,
  );

  function showCellTip(event: PointerEvent) {
    const cell = (event.target as Element).closest<HTMLElement>("[data-i]");
    if (!cell) {
      hoveredCell = null;
      return;
    }
    const rect = cell.getBoundingClientRect();
    // Keep the tooltip (roughly 160px wide) inside the window horizontally.
    const x = Math.min(Math.max(rect.left + rect.width / 2, 90), window.innerWidth - 90);
    hoveredCell = { day: activityDays[Number(cell.dataset.i)], x, y: rect.top };
  }

  function hideCellTip() {
    hoveredCell = null;
  }

  // TODO: replace with the user's real strength (0-100) in each pattern.
  const patterns = [
    { name: "Two Pointers", short: "Two Pointers", value: 82 },
    { name: "Sliding Window", short: "Sliding Window", value: 74 },
    { name: "Binary Search", short: "Binary Search", value: 68 },
    { name: "Stack", short: "Stack", value: 61 },
    { name: "Linked List", short: "Linked List", value: 77 },
    { name: "Trees", short: "Trees", value: 85 },
    { name: "Graphs", short: "Graphs", value: 48 },
    { name: "Heap", short: "Heap", value: 55 },
    { name: "Backtracking", short: "Backtracking", value: 42 },
    { name: "Dynamic Programming", short: "DP", value: 36 },
    { name: "Greedy", short: "Greedy", value: 58 },
    { name: "Intervals", short: "Intervals", value: 64 },
  ];

  // The radar chart is drawn in a fixed coordinate system and scaled to fit its container.
  const RADAR = { w: 440, h: 380, cx: 220, cy: 190, r: 120 };

  function radarPoint(index: number, fraction: number, radius = RADAR.r) {
    const angle = -Math.PI / 2 + (index / patterns.length) * 2 * Math.PI;
    const cos = Math.cos(angle);
    const sin = Math.sin(angle);
    return { x: RADAR.cx + cos * radius * fraction, y: RADAR.cy + sin * radius * fraction, cos, sin };
  }

  function radarPath(fractionAt: (index: number) => number) {
    return (
      patterns
        .map((_, i) => {
          const p = radarPoint(i, fractionAt(i));
          return `${i === 0 ? "M" : "L"}${p.x.toFixed(1)} ${p.y.toFixed(1)}`;
        })
        .join(" ") + " Z"
    );
  }

  const radarRings = [0.25, 0.5, 0.75, 1].map((level) => radarPath(() => level));
  const radarAxes = patterns.map((_, i) => radarPoint(i, 1));
  const radarData = radarPath((i) => patterns[i].value / 100);
  const radarVertices = patterns.map((p, i) => radarPoint(i, p.value / 100));
  const radarLabels = patterns.map((p, i) => {
    const pt = radarPoint(i, 1, RADAR.r + 12);
    return {
      text: p.short,
      x: pt.x,
      y: pt.y,
      anchor: pt.cos > 0.3 ? "start" : pt.cos < -0.3 ? "end" : "middle",
      dy: pt.sin > 0.3 ? "0.8em" : pt.sin < -0.3 ? "0" : "0.35em",
    };
  });

  // TODO: replace with the user's real streak and per-day activity.
  const dailyStreak: number = 12;
  // Oldest first; the last entry is today.
  const weekActivity = [true, false, true, true, true, true, true];

  const DAY_NAMES = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

  // The seven days ending today, labelled with a single letter relative to the current day.
  let streakDays = $derived(
    weekActivity.map((active, i) => {
      const date = new Date(now);
      date.setDate(now.getDate() - (weekActivity.length - 1 - i));
      const name = DAY_NAMES[date.getDay()];
      return { name, letter: name[0], active, today: i === weekActivity.length - 1 };
    }),
  );
</script>

<div class="home">
  <header>
    <h1>{greeting}, {userName}!</h1>
    <p class="subtitle">Ready to start coding?</p>
  </header>

  <div class="cards">
    <div class="card card-a">
      <div class="section section-left">
        <h2 class="metric-title">Problems Solved</h2>
        <p class="metric-value" use:countUp={{ value: problemsSolved, delay: 100 }}></p>
        <div
          class="difficulty-bar"
          role="img"
          aria-label={solvedByDifficulty.map((d) => `${d.count} ${d.label}`).join(", ")}
        >
          {#each solvedByDifficulty.filter((d) => d.count > 0) as d}
            <div class="segment" style="flex-grow: {d.count}; background-color: {d.color}"></div>
          {/each}
        </div>
        <div class="breakdown">
          {#each solvedByDifficulty as d}
            <div class="breakdown-item">
              <div class="breakdown-label">
                <span class="dot" style="background-color: {d.color}"></span>
                {d.label}
              </div>
              <div class="breakdown-values">
                <span class="breakdown-count" use:countUp={{ value: d.count, delay: 200 }}></span>
                <span class="breakdown-percent">{percentOf(d.count)}%</span>
              </div>
            </div>
          {/each}
        </div>
      </div>
      <div class="section section-right-top">
        <h2 class="metric-title">Rating</h2>
        <div class="rating-body">
          <div class="rating-summary">
            <p class="rating-value" use:countUp={{ value: rating, delay: 150 }}></p>
            <p class="rating-change">
              <svg
                class="arrow"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="3"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                {#if weeklyChange >= 0}
                  <path d="M12 19V5" /><path d="m5 12 7-7 7 7" />
                {:else}
                  <path d="M12 5v14" /><path d="m19 12-7 7-7-7" />
                {/if}
              </svg>
              <span>{weeklyChange >= 0 ? "+" : "−"}{Math.abs(weeklyChange)} this week</span>
            </p>
          </div>
          <div class="rating-graph" bind:clientWidth={graphWidth} bind:clientHeight={graphHeight}>
            <svg width={graphWidth} height={graphHeight} role="img" aria-label="Rating history">
              <path class="line" pathLength="1" d={linePath} />
              {#if currentPoint}
                <circle class="dot-current" cx={currentPoint.x} cy={currentPoint.y} r="4.5" />
              {/if}
            </svg>
          </div>
        </div>
      </div>
      <div class="section section-right-bottom">
        <h2 class="metric-title">Daily Streak</h2>
        <div class="rating-body">
          <div class="streak-summary">
            <p class="rating-value" use:countUp={{ value: dailyStreak, delay: 150 }}></p>
            <p class="streak-label">{dailyStreak === 1 ? "day" : "days"}</p>
          </div>
          <ul class="streak-days" aria-label="Activity over the last 7 days">
            {#each streakDays as day, i}
              <li class="streak-day" style="--i: {i}">
                <span
                  class="day-circle"
                  class:done={day.active}
                  class:today={day.today}
                  role="img"
                  aria-label="{day.name}{day.today ? ' (today)' : ''}: {day.active
                    ? 'activity'
                    : 'no activity'}"
                >
                  {#if day.active}
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="#000000"
                      stroke-width="4"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      aria-hidden="true"
                    >
                      <path d="M20 6 9 17l-5-5" />
                    </svg>
                  {/if}
                </span>
                <span class="day-letter" aria-hidden="true">{day.letter}</span>
              </li>
            {/each}
          </ul>
        </div>
      </div>
    </div>
    <div class="card card-b">
      <div class="section card-header">
        <h2 class="panel-title">Sommelier Recommendations</h2>
      </div>
      <div class="section card-body rec-body">
        <ul class="rec-list">
          {#each sortedRecommendations as rec, i}
            <li class="rec-item" style="--i: {i}">
              <div class="rec-title">
                <span class="rec-number">{rec.id}.</span>
                <span class="rec-name">{rec.name}</span>
              </div>
              <div class="rec-tag-cell">
                <span class="rec-tag" class:review={rec.kind === "review"}>
                  {rec.kind === "review" ? "Review" : "New"}
                </span>
              </div>
              <div class="rec-solved">
                {#if rec.kind === "review" && rec.lastSolved}
                  Last solved {formatDate(rec.lastSolved)}
                {/if}
              </div>
              <div class="rec-difficulty-cell">
                <span class="rec-difficulty">
                  <span class="dot" style="background-color: {difficultyColor[rec.difficulty]}"></span>
                  {rec.difficulty}
                </span>
              </div>
              <button
                type="button"
                class="rec-open"
                aria-label="Open problem {rec.id}: {rec.name}"
                onclick={() => openProblem(rec.id)}
              >
                Open
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2.5"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <path d="M5 12h14" /><path d="m12 5 7 7-7 7" />
                </svg>
              </button>
            </li>
          {/each}
        </ul>
      </div>
    </div>
    <div class="card card-c">
      <div class="section card-header activity-header">
        <div class="activity-title">
          <h2 class="panel-title">Activity</h2>
          <p class="activity-summary">
            <span class="num">{totalSubmissions.toLocaleString()}</span>
            {totalSubmissions === 1 ? "submission" : "submissions"} ·
            <span class="num">{activeDayCount.toLocaleString()}</span>
            active {activeDayCount === 1 ? "day" : "days"} in the past year
          </p>
        </div>
        <div class="legend" aria-hidden="true">
          <span>Less</span>
          {#each ACTIVITY_COLORS as color}
            <span class="legend-cell" style="background-color: {color}"></span>
          {/each}
          <span>More</span>
        </div>
      </div>
      <div class="section card-body activity-body">
        <div class="months" aria-hidden="true">
          {#each monthLabels as label}
            <span class="month" style="grid-column: {label.col + 1}">{label.text}</span>
          {/each}
        </div>
        <div
          class="activity-grid"
          role="img"
          aria-label="{totalSubmissions} submissions on {activeDayCount} active days in the past year"
          onpointerover={showCellTip}
          onpointerleave={hideCellTip}
        >
          {#each activityDays as day, i}
            <div
              class="activity-cell"
              data-i={i}
              style="--col: {Math.floor(i / 7)}; background-color: {ACTIVITY_COLORS[day.level]}"
            ></div>
          {/each}
        </div>
      </div>
    </div>
    <div class="card card-d">
      <div class="section card-header">
        <h2 class="panel-title">Pattern Strength</h2>
      </div>
      <div class="section card-body pattern-body">
        <div class="radar">
          <svg
            viewBox="0 0 {RADAR.w} {RADAR.h}"
            preserveAspectRatio="xMidYMid meet"
            role="img"
            aria-label="Radar chart of strength in each pattern"
          >
            {#each radarRings as ring}
              <path class="radar-ring" d={ring} />
            {/each}
            {#each radarAxes as end}
              <line class="radar-axis" x1={RADAR.cx} y1={RADAR.cy} x2={end.x} y2={end.y} />
            {/each}
            <path class="radar-area" d={radarData} />
            {#each radarVertices as v}
              <circle class="radar-vertex" cx={v.x} cy={v.y} r="3" />
            {/each}
            {#each radarLabels as label}
              <text class="radar-label" x={label.x} y={label.y} dy={label.dy} text-anchor={label.anchor}>
                {label.text}
              </text>
            {/each}
          </svg>
        </div>
        <ul class="pattern-bars">
          {#each patterns as p, i}
            <li class="pattern-bar" style="--i: {i}">
              <div class="pattern-bar-head">
                <span class="pattern-name">{p.name}</span>
                <span class="pattern-value">{p.value}</span>
              </div>
              <div class="pattern-track">
                <div class="pattern-fill" style="width: {p.value}%"></div>
              </div>
            </li>
          {/each}
        </ul>
      </div>
    </div>
  </div>

  {#if hoveredCell}
    <div class="cell-tooltip" role="tooltip" style="left: {hoveredCell.x}px; top: {hoveredCell.y}px">
      <div class="cell-tooltip-count">
        {hoveredCell.day.count === 0
          ? "No submissions"
          : `${hoveredCell.day.count} ${hoveredCell.day.count === 1 ? "submission" : "submissions"}`}
      </div>
      <div class="cell-tooltip-date">{formatLongDate(hoveredCell.day.date)}</div>
    </div>
  {/if}
</div>

<style>
  /* Everything on this page is sized in multiples of --u (see +layout.svelte). */
  .home {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: calc(520 * var(--u));
  }

  .cards {
    flex: 1;
    min-height: 0;
    margin-top: calc(20 * var(--u));
    display: grid;
    gap: calc(16 * var(--u));
    grid-template-columns: 3fr 2fr;
    /* The middle card in the left column is slightly taller than the other two. */
    /* minmax(0, …) keeps a card's content from stretching its row. */
    grid-template-rows: minmax(0, 1fr) minmax(0, 1.25fr) minmax(0, 1fr);
  }

  .card-a {
    grid-column: 1;
    grid-row: 1;
    /* Left half is one section; right half is split into top and bottom. */
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
  }

  .section-left {
    grid-column: 1;
    grid-row: 1 / -1;
    border-right: 1px solid var(--card-border);
  }

  .section {
    padding: calc(16 * var(--u)) calc(24 * var(--u));
    min-width: 0;
  }

  .section-right-top,
  .section-right-bottom {
    display: flex;
    flex-direction: column;
  }

  .rating-body {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    gap: calc(20 * var(--u));
  }

  .rating-summary {
    flex: none;
  }

  .rating-value {
    margin: 0;
    font-size: calc(24 * var(--u));
    font-weight: 700;
    line-height: 1.1;
    color: #edeae3;
  }

  .rating-change {
    display: flex;
    align-items: center;
    gap: calc(4 * var(--u));
    margin: calc(2 * var(--u)) 0 0;
    font-size: calc(11 * var(--u));
    line-height: calc(14 * var(--u));
    color: #a8a296;
  }

  .arrow {
    flex: none;
    width: calc(12 * var(--u));
    height: calc(12 * var(--u));
    color: #e9bc22;
  }

  /* The label sits beside the number, and wraps underneath it only when there
     isn't enough room left next to the circles. */
  .streak-summary {
    flex: 1 1 auto;
    min-width: min-content;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: calc(6 * var(--u));
    row-gap: calc(2 * var(--u));
  }

  .streak-label {
    margin: 0;
    font-size: calc(11 * var(--u));
    line-height: calc(14 * var(--u));
    color: #a8a296;
  }

  .streak-days {
    flex: none;
    display: flex;
    justify-content: flex-end;
    gap: calc(14 * var(--u));
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .streak-day {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: calc(5 * var(--u));
  }

  .day-circle {
    position: relative;
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: calc(18 * var(--u));
    height: calc(18 * var(--u));
    border: calc(1.5 * var(--u)) solid #e9bc22;
    border-radius: 50%;
  }

  .day-circle svg {
    width: calc(10 * var(--u));
    height: calc(10 * var(--u));
  }

  .day-circle.done {
    background-color: #e9bc22;
  }

  /* Thin outer ring that draws attention to the current day. */
  .day-circle.today::after {
    content: "";
    position: absolute;
    inset: calc(-5 * var(--u));
    border: calc(1 * var(--u)) solid #e9bc22;
    border-radius: 50%;
  }

  .day-letter {
    font-size: calc(11 * var(--u));
    line-height: calc(14 * var(--u));
    color: #a8a296;
  }

  .rating-graph {
    flex: 1;
    min-width: 0;
    align-self: stretch;
    position: relative;
  }

  .rating-graph svg {
    position: absolute;
    inset: 0;
    overflow: visible;
  }

  .line {
    fill: none;
    stroke: #a8a296;
    stroke-width: calc(2 * var(--u));
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .dot-current {
    fill: #e9bc22;
    r: calc(4.5 * var(--u));
  }

  .metric-title {
    margin: 0;
    font-size: calc(14 * var(--u));
    font-weight: 600;
    line-height: calc(20 * var(--u));
    color: #c9c2b4;
  }

  .metric-value {
    margin: calc(8 * var(--u)) 0 0;
    font-size: calc(36 * var(--u));
    font-weight: 700;
    line-height: 1.1;
    color: #edeae3;
  }

  .difficulty-bar {
    display: flex;
    gap: calc(3 * var(--u));
    height: calc(8 * var(--u));
    margin-top: calc(16 * var(--u));
    border-radius: calc(4 * var(--u));
    overflow: hidden;
  }

  .segment {
    flex-basis: 0;
    flex-shrink: 1;
  }

  .breakdown {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: calc(12 * var(--u));
    margin-top: calc(20 * var(--u));
  }

  .breakdown-label {
    display: flex;
    align-items: center;
    gap: calc(6 * var(--u));
    font-size: calc(12 * var(--u));
    line-height: calc(16 * var(--u));
    color: #c9c2b4;
  }

  .dot {
    flex: none;
    width: calc(8 * var(--u));
    height: calc(8 * var(--u));
    border-radius: 50%;
  }

  .breakdown-values {
    display: flex;
    align-items: baseline;
    gap: calc(6 * var(--u));
    margin-top: calc(4 * var(--u));
  }

  .breakdown-count {
    font-size: calc(18 * var(--u));
    font-weight: 700;
    line-height: 1.2;
    color: #edeae3;
  }

  .breakdown-percent {
    font-size: calc(11 * var(--u));
    color: #a8a296;
  }

  .section-right-top {
    grid-column: 2;
    grid-row: 1;
    border-bottom: 1px solid var(--card-border);
  }

  .section-right-bottom {
    grid-column: 2;
    grid-row: 2;
  }

  .card-b {
    display: flex;
    flex-direction: column;
    min-height: 0;
    grid-column: 1;
    grid-row: 2;
  }

  .card-c {
    display: flex;
    flex-direction: column;
    min-height: 0;
    grid-column: 1;
    grid-row: 3;
  }

  .panel-title {
    margin: 0;
    font-size: calc(16 * var(--u));
    font-weight: 600;
    line-height: calc(20 * var(--u));
    color: #edeae3;
  }

  .activity-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(16 * var(--u));
  }

  .activity-title {
    display: flex;
    align-items: baseline;
    gap: calc(12 * var(--u));
    min-width: 0;
  }

  .activity-summary {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: calc(12 * var(--u));
    line-height: calc(16 * var(--u));
    color: #a8a296;
  }

  .activity-summary .num {
    color: #edeae3;
  }

  .legend {
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(4 * var(--u));
    font-size: calc(11 * var(--u));
    line-height: calc(14 * var(--u));
    color: #a8a296;
  }

  .legend-cell {
    width: calc(10 * var(--u));
    height: calc(10 * var(--u));
    border-radius: calc(2 * var(--u));
  }

  .legend span:first-child {
    margin-right: calc(2 * var(--u));
  }

  .legend span:last-child {
    margin-left: calc(2 * var(--u));
  }

  .activity-body {
    --cell-gap: calc(3 * var(--u));

    display: flex;
    flex-direction: column;
    gap: calc(4 * var(--u));
  }

  .months,
  .activity-grid {
    display: grid;
    grid-template-columns: repeat(53, minmax(0, 1fr));
    column-gap: var(--cell-gap);
  }

  .months {
    flex: none;
    height: calc(14 * var(--u));
    font-size: calc(11 * var(--u));
    line-height: calc(14 * var(--u));
    color: #a8a296;
  }

  .month {
    grid-row: 1;
    white-space: nowrap;
  }

  /* Weeks run down the columns (Sunday at the top); cells stretch to fill the card. */
  .activity-grid {
    flex: 1;
    min-height: 0;
    grid-template-rows: repeat(7, minmax(0, 1fr));
    grid-auto-flow: column;
    row-gap: var(--cell-gap);
  }

  .activity-cell {
    border-radius: calc(2 * var(--u));
  }

  .activity-cell:hover {
    outline: 1px solid rgba(255, 255, 255, 0.5);
    outline-offset: -1px;
  }

  /* Matches the nav tooltips. Positioned in viewport coordinates, above the hovered cell. */
  .cell-tooltip {
    position: fixed;
    z-index: 10;
    transform: translate(-50%, calc(-100% - 8px));
    padding: calc(6 * var(--u)) calc(10 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: calc(8 * var(--u));
    background-color: #14171c;
    box-shadow: 4px 4px 16px rgba(0, 0, 0, 0.4);
    white-space: nowrap;
    pointer-events: none;
  }

  .cell-tooltip-count {
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #edeae3;
  }

  .cell-tooltip-date {
    font-size: calc(11 * var(--u));
    line-height: calc(14 * var(--u));
    color: #a8a296;
  }

  .rec-body {
    display: flex;
    flex-direction: column;
    padding-top: calc(4 * var(--u));
    padding-bottom: calc(4 * var(--u));
  }

  /*
   * The list fills the card but never sizes it (size containment), and shows
   * exactly --rows entries at a time; the rest scroll, snapping a row at a time.
   */
  .rec-list {
    --rows: 4;

    flex: 1;
    min-height: 0;
    container-type: size;
    margin: 0;
    padding: 0;
    list-style: none;
    overflow-y: auto;
    scroll-snap-type: y mandatory;
    /* Still scrolls with the wheel, trackpad, touch and keyboard; just no visible bar. */
    scrollbar-width: none;
  }

  .rec-list::-webkit-scrollbar {
    display: none;
  }

  /* Columns: title | tag | last solved | difficulty | open button */
  .rec-item {
    display: grid;
    grid-template-columns:
      minmax(0, 1fr)
      calc(64 * var(--u))
      calc(112 * var(--u))
      calc(80 * var(--u))
      auto;
    align-items: center;
    column-gap: calc(12 * var(--u));
    box-sizing: border-box;
    height: calc(100cqh / var(--rows));
    border-bottom: 1px solid var(--card-border);
    scroll-snap-align: start;
  }

  .rec-item:last-child {
    border-bottom: none;
  }

  .rec-title {
    display: flex;
    align-items: baseline;
    gap: calc(8 * var(--u));
    min-width: 0;
    font-size: calc(13 * var(--u));
    line-height: calc(20 * var(--u));
  }

  .rec-number {
    flex: none;
    font-variant-numeric: tabular-nums;
    color: #a8a296;
  }

  .rec-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #edeae3;
  }

  .rec-tag {
    display: inline-block;
    padding: calc(2 * var(--u)) calc(8 * var(--u));
    border-radius: calc(999 * var(--u));
    font-size: calc(11 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
  }

  .rec-difficulty {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    font-size: calc(12 * var(--u));
    line-height: calc(16 * var(--u));
    color: #c9c2b4;
  }

  .rec-tag {
    color: #e9bc22;
    background-color: rgba(233, 188, 34, 0.16);
  }

  .rec-tag.review {
    color: #aab1c5;
    background-color: rgba(170, 177, 197, 0.16);
  }

  .rec-solved {
    font-size: calc(11 * var(--u));
    line-height: calc(16 * var(--u));
    color: #a8a296;
    white-space: nowrap;
  }

  .rec-open {
    display: inline-flex;
    align-items: center;
    gap: calc(4 * var(--u));
    padding: calc(4 * var(--u)) calc(10 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: calc(8 * var(--u));
    background: transparent;
    color: #edeae3;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    cursor: pointer;
    transition:
      background-color 0.15s,
      border-color 0.15s;
  }

  .rec-open:hover {
    background-color: rgba(255, 255, 255, 0.06);
    border-color: rgba(255, 255, 255, 0.2);
  }

  .rec-open:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  .rec-open svg {
    width: calc(12 * var(--u));
    height: calc(12 * var(--u));
  }

  .pattern-body {
    display: flex;
    flex-direction: column;
    gap: calc(12 * var(--u));
  }

  .radar {
    flex: 1;
    min-height: 0;
  }

  .radar svg {
    display: block;
    width: 100%;
    height: 100%;
  }

  .radar-ring,
  .radar-axis {
    fill: none;
    stroke: rgba(255, 255, 255, 0.08);
    stroke-width: 1;
  }

  .radar-area {
    fill: rgba(233, 188, 34, 0.18);
    stroke: #e9bc22;
    stroke-width: 2;
    stroke-linejoin: round;
  }

  .radar-vertex {
    fill: #e9bc22;
  }

  .radar-label {
    font-size: 12px;
    fill: #a8a296;
  }

  .pattern-bars {
    flex: none;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: calc(10 * var(--u)) calc(24 * var(--u));
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .pattern-bar-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: calc(8 * var(--u));
    margin-bottom: calc(4 * var(--u));
    font-size: calc(12 * var(--u));
    line-height: calc(16 * var(--u));
  }

  .pattern-name {
    color: #c9c2b4;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pattern-value {
    flex: none;
    font-weight: 400;
    color: #edeae3;
  }

  .pattern-track {
    height: calc(5 * var(--u));
    border-radius: calc(3 * var(--u));
    background-color: rgba(255, 255, 255, 0.08);
    overflow: hidden;
  }

  .pattern-fill {
    height: 100%;
    border-radius: inherit;
    background-color: #e9bc22;
  }

  .card-header {
    flex: none;
    border-bottom: 1px solid var(--card-border);
  }

  .card-body {
    flex: 1;
    min-height: 0;
  }

  .card-d {
    display: flex;
    flex-direction: column;
    grid-column: 2;
    grid-row: 1 / -1;
  }

  h1 {
    margin: 0;
    font-size: calc(28 * var(--u));
    font-weight: 600;
    line-height: 1.2;
  }

  .subtitle {
    margin: calc(6 * var(--u)) 0 0;
    font-size: calc(14 * var(--u));
    color: #a8a296;
  }

  /* ---- Load-in animations ----------------------------------------------------
     `backwards` fill keeps each element in its starting state during its delay, and
     leaves no animation (or transform) behind once it finishes. */

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(calc(14 * var(--u)));
    }
  }

  @keyframes fade-in {
    from {
      opacity: 0;
    }
  }

  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.4);
    }
  }

  @keyframes slide-in {
    from {
      opacity: 0;
      transform: translateX(calc(-12 * var(--u)));
    }
  }

  @keyframes wipe {
    from {
      clip-path: inset(0 100% 0 0);
    }
    to {
      clip-path: inset(0 0 0 0);
    }
  }

  @keyframes grow-x {
    from {
      transform: scaleX(0);
    }
  }

  @keyframes radar-grow {
    from {
      opacity: 0;
      transform: scale(0.15);
    }
  }

  @keyframes draw {
    from {
      stroke-dashoffset: 1;
    }
    to {
      stroke-dashoffset: 0;
    }
  }

  header {
    animation: rise 0.5s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  .card-a,
  .card-b,
  .card-c,
  .card-d {
    animation: rise 0.55s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  .card-a {
    animation-delay: 0.02s;
  }

  .card-d {
    animation-delay: 0.05s;
  }

  .card-b {
    animation-delay: 0.08s;
  }

  .card-c {
    animation-delay: 0.12s;
  }

  /* Problems Solved: the difficulty bar wipes in from the left. */
  .difficulty-bar {
    animation: wipe 0.8s cubic-bezier(0.3, 0.7, 0.2, 1) 0.2s backwards;
  }

  /* Rating: the line draws itself, then the current-point dot pops in. */
  .line {
    stroke-dasharray: 1;
    animation: draw 1s ease-out 0.25s backwards;
  }

  .dot-current {
    transform-box: fill-box;
    transform-origin: center;
    animation: pop 0.45s cubic-bezier(0.3, 1.6, 0.5, 1) 1.1s backwards;
  }

  /* Daily Streak: the circles pop in one after another. */
  .streak-day {
    animation: pop 0.4s cubic-bezier(0.3, 1.5, 0.5, 1) calc(0.3s + var(--i) * 50ms) backwards;
  }

  /* Recommendations: rows slide in one after another. */
  .rec-item {
    animation: slide-in 0.45s cubic-bezier(0.2, 0.7, 0.2, 1) calc(0.25s + var(--i) * 55ms) backwards;
  }

  /* Activity: cells fade in as a wave sweeping left to right. */
  .activity-cell {
    animation: pop 0.45s ease-out calc(0.2s + var(--col) * 8ms) backwards;
  }

  /* Pattern Strength: the radar grows out from its center and the bars fill. */
  .radar-ring,
  .radar-axis {
    animation: fade-in 0.6s ease-out 0.15s backwards;
  }

  .radar-area {
    transform-origin: 220px 190px;
    animation: radar-grow 0.8s cubic-bezier(0.2, 0.7, 0.2, 1) 0.2s backwards;
  }

  .radar-vertex {
    animation: fade-in 0.4s ease-out 0.7s backwards;
  }

  .pattern-fill {
    transform-origin: left center;
    animation: grow-x 0.8s cubic-bezier(0.2, 0.7, 0.2, 1) calc(0.3s + var(--i) * 35ms) backwards;
  }

  @media (prefers-reduced-motion: reduce) {
    .home :global(*) {
      animation: none !important;
    }
  }
</style>
