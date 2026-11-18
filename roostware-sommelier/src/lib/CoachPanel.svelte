<script lang="ts">
  // The AI coach's drawer, on the right of the Solve tab. What it offers follows where the problem
  // stands (see `phaseOf` in coach.svelte.ts): hints and guiding questions while solving, hints
  // and debugging help after a wrong submission, a Big-O analysis after an accepted one, and the
  // full solution once the user gives up. A conversation per problem sits underneath, with chips
  // showing (and letting the user switch off) what the coach is given.
  import { tick } from "svelte";
  import AnalysisCard from "$lib/AnalysisCard.svelte";
  import { coach, contextFor, phaseOf, type CoachMessage, type Phase } from "$lib/coach.svelte";
  import { DEBUG_STEPS, HINT_STEPS, MAX_DEBUG_LEVEL, MAX_HINT_LEVEL } from "$lib/coachProvider";
  import CoachRich from "$lib/CoachRich.svelte";
  import type { ProblemDetail } from "$lib/db";
  import GiveUpButton from "$lib/GiveUpButton.svelte";
  import { coachSetup } from "$lib/coachSetup.svelte";
  import { COACH_NAME } from "$lib/coachName";
  import { compactTokens, costText, usageLine } from "$lib/coachUsage";
  import { leetcode } from "$lib/leetcode.svelte";
  import { settings } from "$lib/settings.svelte";
  import { settingsDialog } from "$lib/settingsDialog.svelte";
  import SolutionCard from "$lib/SolutionCard.svelte";

  let {
    problem,
    lang,
    getCode,
    onclose,
  }: {
    problem: ProblemDetail;
    lang: string;
    getCode: () => string;
    onclose: () => void;
  } = $props();

  let history = $derived(leetcode.stateFor(problem.id).history);
  let info = $derived(coach.giveUpInfo(problem.id));
  let t = $derived(coach.thread(problem.id));
  let p = $derived(phaseOf(history, info, t.freshAfter));
  let ctx = () => contextFor(problem, lang, getCode);

  // Whether the coach has what it needs (a key, a model, an address, the switch on). When it
  // doesn't, the drawer says what's missing and offers the Settings instead of its buttons.
  let ready = $derived(coachSetup.ready);
  let setupNeeded = $derived(coachSetup.status !== null && !coachSetup.ready);
  let total = $derived(coach.usageFor(problem.id));
  let totalCost = $derived(costText(total.input, total.output, settings.coachPriceInput, settings.coachPriceOutput));

  // ---- The state badge ----------------------------------------------------------
  const dateText = (ms: number) => new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });

  const BADGE: Record<Phase, string> = {
    solving: "Solving",
    wrong: "Wrong",
    accepted: "Accepted",
    gaveUp: "Gave up",
  };

  let badgeText = $derived.by(() => {
    switch (p.phase) {
      case "solving":
        return history === null ? "Solving" : "Solving · no submissions yet";
      case "wrong":
        return `${p.latest?.status ?? "Not accepted"} · attempt ${p.failedSince}`;
      case "accepted":
        return "Accepted";
      case "gaveUp": {
        const when = info?.retryAt;
        if (!when || !info || info.recoverable <= 0) return "Gave up";
        return when > Date.now() ? `Gave up · retry for rating from ${dateText(when)}` : "Gave up · a re-solve now counts";
      }
    }
  });

  let badgeTip = $derived.by(() => {
    if (p.phase !== "gaveUp" || !info?.retryAt) return "";
    if (info.recoverable <= 0) return "A later solve doesn't change your rating";
    return info.retryAt > Date.now()
      ? `Solving it again before ${dateText(info.retryAt)} doesn't count toward your rating. After that it wins back up to +${info.recoverable}`
      : `Solving it again now wins back up to +${info.recoverable} rating`;
  });

  // ---- Running the coach --------------------------------------------------------
  // Asked for from outside (the Judge panel's buttons): run it once the drawer has what it needs.
  $effect(() => {
    const req = coach.pending;
    if (!req || req.problemId !== problem.id || history === null) return;
    coach.pending = null;
    void coach.ask(ctx(), req.action);
  });

  // How long the reply being waited for has taken. A card (an analysis, a solution) isn't shown as it
  // arrives, so a slow one would otherwise look exactly like a stuck one.
  let now = $state(Date.now());
  $effect(() => {
    if (!t.busy) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const waited = (m: CoachMessage) => (m.startedAt ? Math.max(0, Math.floor((now - m.startedAt) / 1000)) : 0);

  let composerEl: HTMLTextAreaElement | undefined = $state();
  let draft = $state("");

  function send() {
    const text = draft.trim();
    if (text === "" || t.busy || !ready) return;
    draft = "";
    resize();
    void coach.send(ctx(), text);
  }

  function onComposerKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      send();
    }
  }

  /** Grows the box with what's typed, up to a few lines. */
  function resize() {
    if (!composerEl) return;
    composerEl.style.height = "auto";
    composerEl.style.height = `${Math.min(composerEl.scrollHeight, 120)}px`;
  }

  async function discuss() {
    draft = draft || "About the analysis: ";
    await tick();
    resize();
    composerEl?.focus();
    composerEl?.setSelectionRange(draft.length, draft.length);
  }

  // ---- The thread ---------------------------------------------------------------
  let threadEl: HTMLElement | undefined = $state();
  let stuck = $state(true); // follow new text, unless the user scrolled up to read

  function onThreadScroll() {
    if (!threadEl) return;
    stuck = threadEl.scrollHeight - threadEl.scrollTop - threadEl.clientHeight < 48;
  }

  let lastLength = $derived.by(() => {
    const last = t.messages[t.messages.length - 1];
    return last?.body.kind === "text" ? last.body.text.length : 0;
  });

  $effect(() => {
    void t.messages.length;
    void lastLength;
    void p.phase;
    void tick().then(() => {
      if (threadEl && stuck) threadEl.scrollTop = threadEl.scrollHeight;
    });
  });

  // An analysis asked for twice: bring the one already made into view.
  $effect(() => {
    const id = t.focus;
    if (id === null) return;
    void tick().then(() => {
      threadEl?.querySelector(`[data-message="${id}"]`)?.scrollIntoView({ block: "start", behavior: "smooth" });
      t.focus = null;
    });
  });

  // Switching problems starts at the bottom of that problem's conversation.
  $effect(() => {
    void problem.id;
    stuck = true;
    draft = "";
  });

  // ---- The actions --------------------------------------------------------------
  let hintDone = $derived(t.hintLevel >= MAX_HINT_LEVEL);
  let debugLevel = $derived(p.latest ? (t.debugLevels[p.latest.id] ?? 0) : 0);
  let debugDone = $derived(debugLevel >= MAX_DEBUG_LEVEL);
  let analysisMade = $derived(p.accepted ? t.analyses[p.accepted.id] !== undefined : false);
  let hasSolution = $derived(t.messages.some((m) => m.body.kind === "solution"));

  function messageSubmission(m: CoachMessage) {
    if (m.body.kind !== "analysis") return null;
    const id = m.body.submissionId;
    return history?.find((s) => s.id === id) ?? null;
  }

  const phaseNote: Record<Phase, string> = {
    solving: "Stuck? Ask for a hint, let me ask you questions, or just ask me anything about the problem.",
    wrong: "Not quite. I can help you find the bug. Ask why it failed, ask for a hint, or ask me anything about your code.",
    accepted: "Solved. Want to know how efficient it is, and what to take from it?",
    gaveUp: "You've seen the solution. Ask me anything about it.",
  };

  let sharing = $derived(p.phase === "wrong" || p.phase === "accepted");
</script>

{#snippet sparkle()}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z" />
  </svg>
{/snippet}

{#snippet pips(done: number, total: number)}
  <span class="pips" aria-hidden="true">
    {#each Array(total) as _, i (i)}<span class="pip" class:on={i < done}></span>{/each}
  </span>
{/snippet}

<aside class="coach" class:working={t.busy} aria-label="{COACH_NAME}, the AI coach">
  <header>
    <span class="title">{@render sparkle()}<span class="ai-text">{COACH_NAME}</span><span class="ai-pill">AI</span></span>
    <span class="spacer"></span>
    <span class="tip" data-tip="New conversation">
      <button type="button" class="icon-btn" aria-label="New conversation" disabled={t.messages.length === 0 && !t.questioning} onclick={() => coach.newConversation(problem.id)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
          <path d="M3 3v5h5" />
        </svg>
      </button>
    </span>
    <span class="tip" data-tip="Close {COACH_NAME}">
      <button type="button" class="icon-btn" aria-label="Close {COACH_NAME}" onclick={onclose}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true">
          <path d="M18 6 6 18" />
          <path d="m6 6 12 12" />
        </svg>
      </button>
    </span>
  </header>

  <div class="status">
    <span class="badge tip" class:has-tip={badgeTip !== ""} data-tip={badgeTip} data-phase={p.phase}>
      <span class="dot" aria-hidden="true"></span>
      <span class="sr-only">{BADGE[p.phase]}:</span>
      {badgeText}
    </span>
  </div>

  {#if setupNeeded}
    <div class="setup" role="group" aria-label="Set up {COACH_NAME}">
      <p class="setup-title">Set up {COACH_NAME}</p>
      <p class="setup-text">{coachSetup.reason}</p>
      <button type="button" class="setup-btn" onclick={() => settingsDialog.open("coach")}>Open Settings</button>
    </div>
  {/if}

  <!-- What the coach can do right now. -->
  <div class="actions" role="group" aria-label="Ask {COACH_NAME}">
    {#if !ready}
      <!-- Nothing to offer until it's set up. -->
    {:else if p.phase === "solving" || p.phase === "wrong"}
      {#if p.phase === "wrong" && p.latest}
        <button type="button" class="action" disabled={t.busy || debugDone} onclick={() => coach.ask(ctx(), "debug")}>
          <span class="a-top">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></svg>
            <span class="a-title">{debugLevel === 0 ? "Why did this fail?" : "Go further"}</span>
            {@render pips(debugLevel, MAX_DEBUG_LEVEL)}
          </span>
          <span class="a-sub">{debugDone ? "That's as far as I go without the solution" : `Next: ${DEBUG_STEPS[debugLevel]}`}</span>
        </button>
      {/if}
      <button type="button" class="action" disabled={t.busy || hintDone} onclick={() => coach.ask(ctx(), "hint")}>
        <span class="a-top">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5" /><path d="M9 18h6" /><path d="M10 22h4" /></svg>
          <span class="a-title">{t.hintLevel === 0 ? "Give me a hint" : "Another hint"}</span>
          {@render pips(t.hintLevel, MAX_HINT_LEVEL)}
        </span>
        <span class="a-sub">{hintDone ? "That's as far as hints go. Giving up shows the solution" : `Next: ${HINT_STEPS[t.hintLevel]}`}</span>
      </button>
      {#if p.phase === "solving"}
        <button type="button" class="action" disabled={t.busy || t.questioning} onclick={() => coach.ask(ctx(), "questions")}>
          <span class="a-top">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" /><path d="M12 17h.01" /></svg>
            <span class="a-title">{t.questioning ? "Questions in progress" : "Guide me with questions"}</span>
          </span>
          <span class="a-sub">{t.questioning ? "Answer below" : "I ask, you think"}</span>
        </button>
      {/if}
    {:else if p.phase === "accepted"}
      <button type="button" class="action" disabled={t.busy} onclick={() => coach.ask(ctx(), "analyze")}>
        <span class="a-top">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 7 13.5 15.5 8.5 10.5 2 17" /><path d="M16 7h6v6" /></svg>
          <span class="a-title">{analysisMade ? "Show the analysis" : "Analyze my solution"}</span>
        </span>
        <span class="a-sub">Time and space, what went well, what to improve</span>
      </button>
      <button type="button" class="action" disabled={t.busy} onclick={() => coach.ask(ctx(), "solution")}>
        <span class="a-top">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m16 18 6-6-6-6" /><path d="m8 6-6 6 6 6" /></svg>
          <span class="a-title">Show the optimal solution</span>
        </span>
        <span class="a-sub">Doesn't change your rating or reviews</span>
      </button>
    {:else if !hasSolution}
      <!-- Gave up, but this conversation doesn't have the solution (the app was restarted since). Showing it again logs nothing. -->
      <button type="button" class="action" disabled={t.busy} onclick={() => coach.ask(ctx(), "solution", { viaGiveUp: true })}>
        <span class="a-top">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m16 18 6-6-6-6" /><path d="m8 6-6 6 6 6" /></svg>
          <span class="a-title">Show the solution</span>
        </span>
        <span class="a-sub">You already gave up, so this costs nothing more</span>
      </button>
    {/if}
  </div>

  <!-- The conversation. -->
  <div class="thread" role="log" aria-live="polite" aria-label="Conversation" bind:this={threadEl} onscroll={onThreadScroll}>
    {#if t.messages.length === 0}
      <p class="empty">{phaseNote[p.phase]}</p>
    {/if}

    {#each t.messages as m (m.id)}
      {#if m.body.kind === "notice"}
        <p class="notice" data-message={m.id}>{m.body.text}</p>
      {:else if m.role === "user"}
        <div class="msg user" data-message={m.id}>
          <p>{m.body.kind === "text" ? m.body.text : ""}</p>
        </div>
      {:else}
        <div class="msg coach-msg" data-message={m.id}>
          {#if m.label}
            <div class="label-row"><span class="label ai-text">{m.label}</span></div>
          {/if}

          {#if m.body.kind === "text"}
            {#if m.body.text === "" && m.pending}
              <p class="thinking" role="status">
                <span class="dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="shimmer">Thinking…</span>
                <!-- Kept out of the live region, so a screen reader isn't told the time every second. -->
                {#if waited(m) >= 15}<span class="waited" aria-hidden="true">{waited(m)}s</span>{/if}
              </p>
              {#if waited(m) >= 60}
                <p class="slow">Taking longer than usual. You can stop it and try again.</p>
              {/if}
            {:else}
              <CoachRich text={m.body.text} aboutCode={m.aboutCode} />
            {/if}
          {:else if m.body.kind === "analysis"}
            <AnalysisCard
              analysis={m.body.analysis}
              aboutCode={m.aboutCode}
              submission={messageSubmission(m)}
              busy={t.busy}
              ondiscuss={discuss}
              onoptimal={() => coach.ask(ctx(), "solution")}
              onagain={() => coach.ask(ctx(), "analyze", { again: true })}
            />
          {:else if m.body.kind === "solution"}
            <SolutionCard solution={m.body.solution} aboutCode={m.aboutCode} />
          {/if}

          {#if m.usage && !m.pending}
            <p class="usage">{usageLine(m.usage.inputTokens, m.usage.outputTokens, settings.coachPriceInput, settings.coachPriceOutput)}</p>
          {/if}
          {#if m.pending && m.body.kind === "text" && m.body.text !== ""}
            <span class="caret" aria-hidden="true"></span>
          {/if}
          {#if m.stopped}
            <p class="stopped">Stopped</p>
          {/if}
          {#if m.error}
            <p class="error" role="alert">
              {m.error}
              {#if m.errorKind === "notReady" || m.errorKind === "auth"}
                <button type="button" class="link-btn" onclick={() => settingsDialog.open("coach")}>Open Settings</button>
              {/if}
              {#if m.retry}
                <button type="button" class="link-btn" disabled={t.busy || !ready} onclick={() => coach.retry(ctx(), m.id)}>Try again</button>
              {/if}
            </p>
          {/if}
        </div>
      {/if}
    {/each}
  </div>

  <!-- What goes along with a message, and the box to type in. -->
  <div class="composer">
    {#if t.questioning}
      <div class="questioning">
        <span>Guiding questions: type your answer</span>
        <button type="button" class="link-btn" onclick={() => coach.endQuestions(problem.id)}>End</button>
      </div>
    {/if}

    <div class="chips" role="group" aria-label="What {COACH_NAME} can see">
      <span class="chip fixed"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>Problem</span>
      <span class="tip" data-tip={t.shareCode ? "The code in your editor is sent with your questions" : "Your code isn't sent"}>
        <button type="button" class="chip" aria-pressed={t.shareCode} onclick={() => coach.setShare(problem.id, "code", !t.shareCode)}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{#if t.shareCode}<path d="M20 6 9 17l-5-5" />{:else}<path d="M18 6 6 18" /><path d="m6 6 12 12" />{/if}</svg>
          My code
        </button>
      </span>
      {#if p.latest}
        <span class="tip" data-tip={sharing ? "Debugging and analysis always use the submission they're about" : t.shareSubmission ? "The verdict of your latest submission is sent too" : "Your submissions aren't sent"}>
          <button type="button" class="chip" aria-pressed={sharing || t.shareSubmission} disabled={sharing} onclick={() => coach.setShare(problem.id, "submission", !t.shareSubmission)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{#if sharing || t.shareSubmission}<path d="M20 6 9 17l-5-5" />{:else}<path d="M18 6 6 18" /><path d="m6 6 12 12" />{/if}</svg>
            Last submission · {p.latest.status}
          </button>
        </span>
      {/if}
    </div>

    <div class="box" class:working={t.busy}>
      <textarea
        bind:this={composerEl}
        bind:value={draft}
        rows="1"
        spellcheck="true"
        aria-label={t.questioning ? "Your answer" : `Ask ${COACH_NAME}`}
        disabled={!ready}
        placeholder={!ready ? `Set up ${COACH_NAME} to ask for help` : t.questioning ? "Answer the question…" : `Ask ${COACH_NAME} anything…`}
        oninput={resize}
        onkeydown={onComposerKeydown}
      ></textarea>
      {#if t.busy}
        <span class="tip" data-tip="Stop">
          <button type="button" class="send stop" aria-label="Stop the reply" onclick={() => coach.stop(problem.id)}>
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2" /></svg>
          </button>
        </span>
      {:else}
        <span class="tip" data-tip="Send (Enter)">
          <button type="button" class="send" aria-label="Send" disabled={draft.trim() === "" || !ready} onclick={send}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5" /><path d="m5 12 7-7 7 7" /></svg>
          </button>
        </span>
      {/if}
    </div>

    <div class="foot">
      <GiveUpButton {problem} {lang} {getCode} variant="link" />
      {#if total.input + total.output > 0}
        <span class="total tip" data-tip="Everything {COACH_NAME} has used on this problem, including earlier conversations">
          {compactTokens(total.input + total.output)} tokens{totalCost ? ` · ${totalCost}` : ""}
        </span>
      {/if}
    </div>
  </div>
</aside>

<style>
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  @keyframes bounce {
    0%,
    80%,
    100% {
      opacity: 0.3;
    }
    40% {
      opacity: 1;
    }
  }

  .coach {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
  }

  /* One gradient line under the header marks the panel as AI. While a reply is being written it
     thickens and flows. */
  header::after {
    content: "";
    position: absolute;
    right: 0;
    bottom: -1px;
    left: 0;
    height: 1px;
    background: var(--ai-flow);
    background-size: 200% 100%;
    opacity: 0.4;
    animation: ai-flow var(--ai-drift) linear infinite;
    transition:
      height 0.25s,
      opacity 0.25s;
  }

  .working header::after {
    height: 2px;
    opacity: 0.85;
    animation: ai-flow 4s linear infinite;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  /* ---- Header and state badge ---- */
  header {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(6 * var(--u));
    padding: calc(10 * var(--u)) calc(10 * var(--u)) calc(10 * var(--u)) calc(14 * var(--u));
    border-bottom: 1px solid var(--card-border);
  }

  .title {
    display: inline-flex;
    align-items: center;
    gap: calc(7 * var(--u));
    font-size: calc(14 * var(--u));
    font-weight: 600;
    color: #edeae3;
  }

  /* A small "AI" next to the name, so it's always clear what Vin is. */
  .ai-pill {
    padding: 0 calc(6 * var(--u));
    border: 1px solid var(--ai-border);
    border-radius: calc(999 * var(--u));
    font-size: calc(10 * var(--u));
    font-weight: 600;
    line-height: calc(15 * var(--u));
    letter-spacing: 0.06em;
    color: var(--ai-solid);
  }

  .title svg {
    width: calc(16 * var(--u));
    height: calc(16 * var(--u));
    stroke: url(#ai-gradient);
  }

  .spacer {
    flex: 1;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: calc(28 * var(--u));
    height: calc(28 * var(--u));
    padding: 0;
    border: 0;
    border-radius: calc(8 * var(--u));
    background: none;
    color: #a8a296;
    cursor: pointer;
    transition:
      background-color 0.15s,
      color 0.15s;
  }

  .icon-btn svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
  }

  .icon-btn:hover:not(:disabled) {
    background-color: rgba(255, 255, 255, 0.07);
    color: #edeae3;
  }

  .icon-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .status {
    flex: none;
    padding: calc(10 * var(--u)) calc(14 * var(--u)) 0;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: calc(7 * var(--u));
    max-width: 100%;
    padding: calc(3 * var(--u)) calc(11 * var(--u)) calc(3 * var(--u)) calc(9 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #1d2027;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #c9c2b4;
  }

  .dot {
    flex: none;
    width: calc(8 * var(--u));
    height: calc(8 * var(--u));
    border-radius: 50%;
    background-color: #6f7a94;
  }

  .badge[data-phase="wrong"] .dot {
    background-color: #ff6b6b;
  }

  .badge[data-phase="accepted"] .dot {
    background-color: #3fae7f;
  }

  .badge[data-phase="gaveUp"] .dot {
    background-color: #e9bc22;
  }

  /* ---- The actions ---- */
  .actions {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: calc(6 * var(--u));
    padding: calc(10 * var(--u)) calc(14 * var(--u)) 0;
  }

  .actions:empty {
    display: none;
  }

  .action {
    display: flex;
    flex-direction: column;
    gap: calc(2 * var(--u));
    padding: calc(8 * var(--u)) calc(12 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(10 * var(--u));
    background-color: #1d2027;
    font: inherit;
    text-align: left;
    color: #edeae3;
    cursor: pointer;
    transition:
      border-color 0.2s,
      background-color 0.15s;
  }

  .action:hover:not(:disabled) {
    border-color: var(--ai-border-strong);
    background-color: #22262e;
  }

  .action:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .a-top {
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
  }

  .a-top svg {
    flex: none;
    width: calc(15 * var(--u));
    height: calc(15 * var(--u));
    color: var(--ai-solid);
  }

  .a-title {
    flex: 1;
    min-width: 0;
    font-size: calc(13 * var(--u));
    font-weight: 600;
    line-height: calc(18 * var(--u));
  }

  .a-sub {
    padding-left: calc(23 * var(--u));
    font-size: calc(11.5 * var(--u));
    line-height: calc(16 * var(--u));
    color: #8d877b;
  }

  .pips {
    display: inline-flex;
    gap: calc(4 * var(--u));
  }

  .pip {
    width: calc(7 * var(--u));
    height: calc(7 * var(--u));
    border: 1px solid var(--ai-border-strong);
    border-radius: 50%;
  }

  .pip.on {
    border-color: transparent;
    background: var(--ai-flow);
    background-size: 200% 100%;
    animation: ai-flow var(--ai-drift) linear infinite;
  }

  /* ---- The conversation ---- */
  .thread {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: calc(14 * var(--u));
    padding: calc(14 * var(--u));
    overflow-x: hidden;
    overflow-y: auto;
  }

  .empty {
    margin: auto 0;
    padding: 0 calc(6 * var(--u));
    font-size: calc(12.5 * var(--u));
    line-height: calc(19 * var(--u));
    text-align: center;
    color: #8d877b;
  }

  .msg p {
    margin: 0;
  }

  .msg.user {
    align-self: flex-end;
    max-width: 88%;
    padding: calc(7 * var(--u)) calc(12 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: calc(12 * var(--u)) calc(12 * var(--u)) calc(3 * var(--u)) calc(12 * var(--u));
    background-color: rgba(255, 255, 255, 0.06);
    font-size: calc(13 * var(--u));
    line-height: calc(19 * var(--u));
    color: #edeae3;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .coach-msg {
    min-width: 0;
  }

  .label-row {
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
    margin-bottom: calc(4 * var(--u));
  }

  .label {
    font-size: calc(11 * var(--u));
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .notice {
    margin: 0;
    font-size: calc(11.5 * var(--u));
    text-align: center;
    color: #8d877b;
  }

  .thinking {
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
    font-size: calc(12.5 * var(--u));
    color: #8d877b;
  }

  .dots {
    display: inline-flex;
    gap: calc(3 * var(--u));
  }

  .dots i {
    width: calc(5 * var(--u));
    height: calc(5 * var(--u));
    border-radius: 50%;
    background-color: var(--ai-1);
    animation: bounce 1.2s infinite;
  }

  .dots i:nth-child(2) {
    background-color: var(--ai-2);
    animation-delay: 0.15s;
  }

  .dots i:nth-child(3) {
    background-color: var(--ai-3);
    animation-delay: 0.3s;
  }

  .waited {
    font-size: calc(11.5 * var(--u));
    font-variant-numeric: tabular-nums;
    color: #6f6a60;
  }

  .slow {
    margin-top: calc(4 * var(--u));
    font-size: calc(12 * var(--u));
    color: #8d877b;
  }

  /* "Thinking…" with the gradient sliding through the letters. */
  .shimmer {
    background: var(--ai-flow);
    background-size: 200% 100%;
    background-clip: text;
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    animation: ai-flow 3.6s linear infinite;
  }

  .caret {
    display: inline-block;
    width: calc(7 * var(--u));
    height: calc(14 * var(--u));
    margin-top: calc(4 * var(--u));
    border-radius: 2px;
    background: var(--ai-flow-v);
    background-size: 100% 200%;
    vertical-align: text-bottom;
    animation:
      blink 1s steps(2, start) infinite,
      ai-flow-y var(--ai-drift) linear infinite;
  }

  .stopped {
    margin-top: calc(4 * var(--u));
    font-size: calc(11.5 * var(--u));
    color: #8d877b;
  }

  .error {
    margin-top: calc(6 * var(--u));
    font-size: calc(12 * var(--u));
    line-height: calc(17 * var(--u));
    color: #ff8a8a;
  }

  .link-btn {
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: #edeae3;
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }

  .link-btn + .link-btn {
    margin-left: calc(10 * var(--u));
  }

  .link-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* ---- The composer ---- */
  .composer {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: calc(8 * var(--u));
    padding: calc(10 * var(--u)) calc(14 * var(--u)) calc(10 * var(--u));
    border-top: 1px solid var(--card-border);
  }

  .questioning {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: calc(5 * var(--u)) calc(10 * var(--u));
    border-radius: calc(8 * var(--u));
    background-color: var(--ai-tint);
    font-size: calc(12 * var(--u));
    color: var(--ai-solid);
  }

  .questioning .link-btn {
    color: var(--ai-solid);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: calc(6 * var(--u));
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: calc(5 * var(--u));
    padding: calc(2 * var(--u)) calc(10 * var(--u)) calc(2 * var(--u)) calc(7 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #181b21;
    font: inherit;
    font-size: calc(11.5 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #8d877b;
  }

  button.chip {
    cursor: pointer;
    transition:
      border-color 0.2s,
      color 0.15s;
  }

  .chip svg {
    width: calc(11 * var(--u));
    height: calc(11 * var(--u));
  }

  .chip.fixed,
  .chip[aria-pressed="true"] {
    color: #c9c2b4;
  }

  .chip[aria-pressed="true"] svg,
  .chip.fixed svg {
    color: #3fae7f;
  }

  .chip[aria-pressed="false"] svg {
    color: #ff8a8a;
  }

  button.chip:hover:not(:disabled) {
    border-color: rgba(255, 255, 255, 0.2);
    color: #edeae3;
  }

  button.chip:disabled {
    cursor: default;
  }

  .box {
    display: flex;
    align-items: flex-end;
    gap: calc(6 * var(--u));
    padding: calc(6 * var(--u)) calc(6 * var(--u)) calc(6 * var(--u)) calc(12 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(12 * var(--u));
    background-color: #1d2027;
    transition: border-color 0.2s;
  }

  .box:focus-within {
    border-color: var(--ai-border-strong);
  }

  /* While a reply is being written the box's border is the gradient, flowing around it. */
  .box.working {
    border-color: transparent;
    background:
      linear-gradient(#1d2027, #1d2027) padding-box,
      var(--ai-flow) border-box;
    background-size:
      auto,
      200% 100%;
    animation: ai-flow-border 4s linear infinite;
  }

  textarea {
    flex: 1;
    min-width: 0;
    max-height: calc(120 * var(--u));
    padding: calc(5 * var(--u)) 0;
    border: 0;
    outline: 0;
    background: none;
    font: inherit;
    font-size: calc(13 * var(--u));
    line-height: calc(19 * var(--u));
    color: #edeae3;
    resize: none;
  }

  textarea::placeholder {
    color: #6f6a60;
  }

  textarea:disabled {
    cursor: not-allowed;
  }

  .send {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: calc(30 * var(--u));
    height: calc(30 * var(--u));
    padding: 0;
    border: 0;
    border-radius: calc(9 * var(--u));
    background: var(--ai-flow);
    background-size: 200% 100%;
    color: #10131a;
    cursor: pointer;
    animation: ai-flow var(--ai-drift) linear infinite;
    transition:
      filter 0.15s,
      opacity 0.2s;
  }

  .send svg {
    width: calc(15 * var(--u));
    height: calc(15 * var(--u));
  }

  .send:hover:not(:disabled) {
    filter: brightness(1.12);
  }

  .send:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .send.stop {
    background: #3a3f4b;
    color: #edeae3;
    animation: none;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(10 * var(--u));
    min-height: 0;
  }

  .total {
    margin-left: auto;
    font-size: calc(11.5 * var(--u));
    font-variant-numeric: tabular-nums;
    color: #8d877b;
  }

  .usage {
    margin: calc(6 * var(--u)) 0 0;
    font-size: calc(11 * var(--u));
    font-variant-numeric: tabular-nums;
    color: #6f6a60;
  }

  /* ---- Not set up yet ---- */
  .setup {
    flex: none;
    margin: calc(10 * var(--u)) calc(14 * var(--u)) 0;
    padding: calc(12 * var(--u)) calc(14 * var(--u));
    border: 1px solid var(--ai-border);
    border-radius: calc(10 * var(--u));
    background-color: var(--ai-tint);
  }

  .setup p {
    margin: 0;
  }

  .setup-title {
    font-size: calc(13 * var(--u));
    font-weight: 600;
    color: #edeae3;
  }

  .setup-text {
    margin-top: calc(3 * var(--u));
    font-size: calc(12.5 * var(--u));
    line-height: calc(18 * var(--u));
    color: #c9c2b4;
  }

  .setup-btn {
    margin-top: calc(10 * var(--u));
    padding: calc(6 * var(--u)) calc(14 * var(--u));
    border: 1px solid transparent;
    border-radius: calc(8 * var(--u));
    background: var(--ai-flow);
    background-size: 200% 100%;
    font: inherit;
    font-size: calc(12.5 * var(--u));
    font-weight: 600;
    color: #10131a;
    cursor: pointer;
    animation: ai-flow var(--ai-drift) linear infinite;
  }

  .setup-btn:hover {
    filter: brightness(1.12);
  }

  .foot:empty {
    display: none;
  }

  button:focus-visible,
  textarea:focus-visible {
    outline: 2px solid var(--ai-solid);
    outline-offset: 2px;
  }

  textarea:focus-visible {
    outline: 0;
  }

  /* Custom tooltips (like the rest of the app). They open downward from the header and upward
     from the composer, aligned to the right edge so the narrow drawer can't clip them. */
  .tip {
    position: relative;
    display: inline-flex;
  }

  .tip[data-tip]:not([data-tip=""])::after {
    content: attr(data-tip);
    position: absolute;
    z-index: 6;
    right: 0;
    width: max-content;
    max-width: calc(260 * var(--u));
    padding: calc(5 * var(--u)) calc(10 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: calc(8 * var(--u));
    background-color: #12141a;
    box-shadow: 0 calc(6 * var(--u)) calc(16 * var(--u)) rgba(0, 0, 0, 0.35);
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    letter-spacing: normal;
    text-transform: none;
    color: #edeae3;
    white-space: normal;
    text-align: left;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.15s;
  }

  header .tip::after,
  .status .tip::after {
    top: calc(100% + 8 * var(--u));
  }

  .status .tip::after {
    right: auto;
    left: 0;
  }

  .box .tip::after {
    bottom: calc(100% + 10 * var(--u));
  }

  .chips .tip::after {
    bottom: calc(100% + 8 * var(--u));
    right: auto;
    left: 0;
  }

  .tip:hover::after,
  .tip:has(:focus-visible)::after {
    opacity: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .dots i,
    .caret,
    .shimmer,
    header::after,
    .working header::after,
    .box.working,
    .pip.on,
    .send,
    .setup-btn {
      animation: none;
    }

    .tip::after {
      transition: none;
    }
  }
</style>
