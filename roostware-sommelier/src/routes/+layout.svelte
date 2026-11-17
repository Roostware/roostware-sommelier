<script lang="ts">
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import SettingsMenu from "$lib/SettingsMenu.svelte";
  import TitleBar from "$lib/TitleBar.svelte";
  import { isMac } from "$lib/platform";
  import { coachSetup } from "$lib/coachSetup.svelte";
  import { loadSettings } from "$lib/settings.svelte";
  import "@fontsource/ibm-plex-sans/latin-400.css";
  import "@fontsource/ibm-plex-sans/latin-500.css";
  import "@fontsource/ibm-plex-sans/latin-600.css";
  import "@fontsource/ibm-plex-sans/latin-700.css";
  import "@fontsource/cascadia-code/latin-400.css";
  import "@fontsource/cascadia-code/latin-400-italic.css"; // comments in the editor
  import "@fontsource/cascadia-code/latin-700.css";

  let { children } = $props();

  // The icons' gradient is animated in the SVG itself, which CSS can't switch off.
  let aiDefs: SVGSVGElement | undefined = $state();

  onMount(() => {
    void loadSettings();
    void coachSetup.refresh();
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) aiDefs?.pauseAnimations();
  });

  const navItems = [
    {
      href: "/",
      label: "Home",
      icon: `<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>`,
    },
    {
      href: "/problems",
      label: "Problems",
      icon: `<path d="M3 5h.01"/><path d="M3 12h.01"/><path d="M3 19h.01"/><path d="M8 5h13"/><path d="M8 12h13"/><path d="M8 19h13"/>`,
    },
    {
      href: "/solve",
      label: "Solve",
      icon: `<path d="m18 16 4-4-4-4"/><path d="m6 8-4 4 4 4"/><path d="m14.5 4-5 16"/>`,
    },
    // (There was a "Learn" tab after Solve, at /learn, that only held a placeholder page. It was
    // taken out for now; it's in the first commit if it comes back: `git show 12fe3bc:./src/routes/learn/+page.svelte`.)
  ];

  function isActive(href: string) {
    const path = page.url.pathname;
    return href === "/" ? path === "/" : path === href || path.startsWith(href + "/");
  }

  let activeIndex = $derived(navItems.findIndex((item) => isActive(item.href)));
</script>

<!-- The AI gradient, for icons to stroke with (`stroke: url(#ai-gradient)`). Must stay in the page, not display:none. -->
<svg class="ai-defs" bind:this={aiDefs} width="0" height="0" aria-hidden="true" focusable="false">
  <defs>
    <!-- "reflect" plus a slide of exactly two gradient lengths loops without a seam: the colours drift
         slowly through the icons, in step with the CSS gradients (same 28s as --ai-drift). -->
    <linearGradient id="ai-gradient" x1="0" y1="0" x2="1" y2="1" spreadMethod="reflect">
      <stop offset="0" style="stop-color: var(--ai-1)" />
      <stop offset="0.5" style="stop-color: var(--ai-2)" />
      <stop offset="1" style="stop-color: var(--ai-3)" />
      <animateTransform attributeName="gradientTransform" type="translate" from="0 0" to="2 2" dur="28s" repeatCount="indefinite" />
    </linearGradient>
  </defs>
</svg>

<main>
  <TitleBar />
  <img class="logo" class:mac={isMac} src="/sommelier.svg" alt="Sommelier" draggable="false" />

  <nav aria-label="Main" style="--i: {Math.max(activeIndex, 0)}">
    <div class="indicator" class:hidden={activeIndex < 0}></div>
    {#each navItems as item}
      <a
        href={item.href}
        class:active={isActive(item.href)}
        aria-current={isActive(item.href) ? "page" : undefined}
      >
        <svg
          viewBox="0 0 24 24"
          width="20"
          height="20"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          {@html item.icon}
        </svg>
        <span class="tooltip" role="tooltip">{item.label}</span>
      </a>
    {/each}
  </nav>

  <SettingsMenu />

  <section class="dashboard">
    {@render children()}
  </section>
</main>

<style>
  /* Shared card style: used by the tab menu and any `.card` element on a page. */
  :global(:root) {
    --card-bg: #262a33;
    --card-border: rgba(255, 255, 255, 0.06);
    --card-radius: 14px;
    --card-shadow: 4px 4px 16px rgba(0, 0, 0, 0.4);
  }

  /* AI features (the coach, its buttons, the editor underlines) share one look: a "pearl" gradient
     of steel, pearl and sand, so it's clear at a glance what comes from the AI. It's nearly
     colourless on purpose: it sits beside the app's gold accent and its green and red status colours
     without competing with them, and the slow movement does most of the signalling. `--ai-flow` loops
     seamlessly (it ends on the colour it starts with), so it can slide forever; it drifts slowly at
     rest and flows faster while the AI is working. */
  :global(:root) {
    --ai-1: #9aa6c0;
    --ai-2: #ede7dc;
    --ai-3: #d8b98a;
    --ai-solid: #b8c0d4; /* one colour for things that can't carry a gradient: text, outlines */
    --ai-gradient: linear-gradient(90deg, var(--ai-1), var(--ai-2), var(--ai-3));
    --ai-flow: linear-gradient(90deg, var(--ai-1), var(--ai-2), var(--ai-3), var(--ai-2), var(--ai-1));
    --ai-flow-v: linear-gradient(180deg, var(--ai-1), var(--ai-2), var(--ai-3), var(--ai-2), var(--ai-1));
    /* How long the colours take to drift once round when the AI is idle. While it's working they
       flow several times faster (a few seconds), so the two states still look different. */
    --ai-drift: 28s;
    --ai-tint: rgba(184, 192, 212, 0.08);
    --ai-tint-strong: rgba(184, 192, 212, 0.16);
    --ai-border: rgba(184, 192, 212, 0.32);
    --ai-border-strong: rgba(184, 192, 212, 0.58);
  }

  /* Slides a `--ai-flow` background (sized 200% wide) by exactly one period, so it never jumps.
     The second is for a border drawn as a second background layer under a solid padding layer. */
  @keyframes -global-ai-flow {
    to {
      background-position-x: 200%;
    }
  }

  @keyframes -global-ai-flow-y {
    to {
      background-position-y: 200%;
    }
  }

  @keyframes -global-ai-flow-border {
    from {
      background-position-x: 0, 0;
    }
    to {
      background-position-x: 0, 200%;
    }
  }

  :global(.ai-defs) {
    position: absolute;
    pointer-events: none;
  }

  /* Gradient text, for labels. Falls back to the solid colour where background-clip: text isn't supported. */
  :global(.ai-text) {
    color: var(--ai-solid);
    background: var(--ai-flow);
    background-size: 200% 100%;
    background-clip: text;
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    animation: ai-flow var(--ai-drift) linear infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.ai-text) {
      animation: none;
    }
  }

  /* Slim, dark scrollbars that match the cards. (The ::-webkit-scrollbar pieces
     only apply while the standard scrollbar-width/scrollbar-color are unset.) */
  :global(::-webkit-scrollbar) {
    width: 8px;
    height: 8px;
  }

  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(::-webkit-scrollbar-thumb) {
    border: 2px solid transparent;
    border-radius: 999px;
    background-color: rgba(255, 255, 255, 0.14);
    background-clip: content-box;
  }

  :global(::-webkit-scrollbar-thumb:hover) {
    background-color: rgba(255, 255, 255, 0.26);
  }

  :global(::-webkit-scrollbar-corner) {
    background: transparent;
  }

  :global(.card) {
    box-sizing: border-box;
    border: 1px solid var(--card-border);
    border-radius: var(--card-radius);
    background-color: var(--card-bg);
    box-shadow: var(--card-shadow);
  }

  :global(button),
  :global(input),
  :global(select),
  :global(textarea) {
    font-family: inherit;
  }

  :global(:root) {
    /* The bundled Cascadia Code, with the system fonts only as a fallback. */
    --font-code: "Cascadia Code", "Cascadia Mono", Consolas, "Courier New", monospace;
  }

  :global(html),
  :global(body) {
    margin: 0;
    min-height: 100vh;
    background-color: #1d2027;
    font-family: "IBM Plex Sans", sans-serif;
    color: #e6e8ee;
  }

  main {
    --strip: 85px;

    position: relative;
    min-height: 100vh;
    background-color: #1d2027;
    overflow: hidden;
  }

  .logo {
    position: absolute;
    top: calc((var(--strip) - 40px) / 2);
    left: 30px;
    height: 40px;
    width: auto;
    /* Let clicks and drags fall through to the title bar's drag region. */
    pointer-events: none;
    user-select: none;
  }

  /* Make room for the native macOS traffic-light buttons. */
  .logo.mac {
    left: 100px;
  }

  nav {
    --item-w: 36px;
    --item-h: 36px;
    --item-gap: 6px;
    --card-pad: 5px;

    position: absolute;
    z-index: 2;
    top: calc(var(--strip) + 8px);
    left: calc((var(--strip) - var(--item-w) - 2 * var(--card-pad) - 2px) / 2);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--item-gap);
    padding: var(--card-pad);
    box-sizing: border-box;
    border: 1px solid var(--card-border);
    border-radius: var(--card-radius);
    background-color: var(--card-bg);
    box-shadow: var(--card-shadow);
  }

  .indicator {
    position: absolute;
    top: var(--card-pad);
    left: var(--card-pad);
    width: var(--item-w);
    height: var(--item-h);
    border-radius: 12px;
    background-color: #e9bc22;
    transform: translateY(calc(var(--i) * (var(--item-h) + var(--item-gap))));
    transition:
      transform 0.35s cubic-bezier(0.34, 1.3, 0.64, 1),
      opacity 0.2s;
  }

  .indicator.hidden {
    opacity: 0;
  }

  nav a {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    width: var(--item-w);
    height: var(--item-h);
    border-radius: 12px;
    color: #8b93a7;
    text-decoration: none;
    transition:
      color 0.25s,
      background-color 0.15s;
  }

  .tooltip {
    position: absolute;
    left: calc(100% + 16px);
    top: 50%;
    transform: translate(-4px, -50%);
    padding: 6px 10px;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.06);
    background-color: #14171c;
    box-shadow: 4px 4px 16px rgba(0, 0, 0, 0.4);
    color: #e6e8ee;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transition:
      opacity 0.15s,
      transform 0.15s;
  }

  nav a:hover .tooltip,
  nav a:focus-visible .tooltip {
    opacity: 1;
    transform: translate(0, -50%);
  }

  nav a:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  nav a:hover {
    color: #e6e8ee;
    background-color: rgba(255, 255, 255, 0.05);
  }

  nav a.active,
  nav a.active:hover {
    color: #1d2027;
    background-color: transparent;
  }

  /* App load-in: the shell fades up, the nav card slides in from the left. */
  @keyframes shell-fade {
    from {
      opacity: 0;
    }
  }

  @keyframes nav-in {
    from {
      opacity: 0;
      transform: translateX(-14px);
    }
  }

  .logo {
    animation: shell-fade 0.6s ease-out backwards;
  }

  nav {
    animation: nav-in 0.5s cubic-bezier(0.2, 0.7, 0.2, 1) 0.05s backwards;
  }

  .dashboard {
    animation: shell-fade 0.5s ease-out backwards;
  }

  @media (prefers-reduced-motion: reduce) {
    .indicator {
      transition: none;
    }

    .logo,
    nav,
    .dashboard {
      animation: none;
    }
  }

  .dashboard {
    /*
     * Page content is sized in multiples of --u, which is 1px at the default window
     * size (1440x900) and grows with the window, up to 1.625px. The smaller of the
     * width- and height-based scales is used so content never outgrows its cards
     * when the window is wide but short (or vice versa).
     */
    --u: calc(
      clamp(
          16px,
          min(calc(16px + (100vw - 1440px) * 0.01), calc(16px + (100vh - 900px) * 0.02)),
          26px
        ) / 16
    );

    position: absolute;
    top: var(--strip);
    left: var(--strip);
    right: 0;
    bottom: 0;
    padding: 40px;
    box-sizing: border-box;
    overflow: auto;
    border-top-left-radius: 25px;
    background: linear-gradient(
      to bottom,
      rgba(35, 40, 55, 1) 0%,
      rgba(29, 32, 39, 0.5) 45%,
      rgba(20, 23, 28, 0) 100%
    );
  }
</style>
