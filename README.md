<h1 align="center">🍷 Sommelier</h1>

<p align="center">
  <strong>A desktop LeetCode trainer that picks what you should practice next, rates your progress like a chess player, and has an AI coach who won't just hand you the answer.</strong>
</p>

<p align="center">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white">
  <img alt="Svelte 5" src="https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-backend-DEA584?logo=rust&logoColor=black">
  <img alt="SQLite" src="https://img.shields.io/badge/SQLite-local--first-003B57?logo=sqlite&logoColor=white">
  <img alt="Platforms" src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS-lightgrey">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
</p>

---

Grinding random problems is a slow way to get good. Sommelier treats interview prep like a training plan: it knows which patterns you're weak on, schedules problems to come back right before you'd forget them, and measures your improvement with a rating that actually means something.

You write and submit code in the app (Monaco editor, real LeetCode judge), and everything else — what to solve, when to revisit it, how you're trending, when to ask for help — is handled for you. All of your data stays on your machine.

## Features

### 🎯 A daily set that adapts to you
- **Spaced repetition with [FSRS-6](https://github.com/open-spaced-repetition/fsrs4anki).** Problems you've solved come back when you're about to forget them, not on a fixed calendar.
- **17 tracked patterns**, from *Arrays & Hashing* up through *Dynamic Programming* and *Advanced Graphs*, mapped from LeetCode's topic tags. Later patterns open up as you build a base in the ones they depend on.
- **Graded on how the solve went, not just whether it passed.** Failed attempts, struggling, or needing the solution all shape the grade that feeds the review schedule (Again / Hard / Good / Easy).
- **Curated lists as a nudge, not a cage.** Blind 75 and the NeetCode lists get a soft bonus when picking new problems; nothing is ever restricted to them.
- **Daily set size is yours to choose** (3 by default).

### ♟️ A rating that works like Elo
- You start at 1000 and play against **fixed problem ratings**. A clean solve counts as a full win (1.0), a struggle counts as a partial one (0.55), and there's no ceiling.
- Only **first solves** move your rating, so you can't farm points by re-doing easy problems. A better re-solve recovers 75% of an earlier loss.
- **Giving up counts as a loss**, and you wait 3 days before the re-solve can recover it. Honest ratings, no padding.
- Provisional ratings for your first games, and a weekly change readout on the dashboard.

### 🤖 Vin, the AI coach
- A side drawer that knows the problem you're on and your code. **Vin nudges, asks questions, and explains, but never writes the solution** unless you've explicitly given up on the problem.
- **Bring your own model:** Anthropic, OpenAI, or any OpenAI-compatible endpoint (local models included). Requests go straight from the Rust backend to the provider.
- API keys live in the **OS credential store** (Windows Credential Manager / macOS Keychain), never in a file.
- Streaming replies with a Stop button, saved chat threads per problem, and a usage and cost tracker.
- Vin is separate from the problem's built-in hints, which stay hidden until you press **Hint**.

### 💻 A proper solving workspace
- Monaco editor with per-language drafts that **autosave**.
- Run and **submit to LeetCode** directly. The in-app sign-in window handles login, and the session is stored in the OS credential store.
- Full problem statements (sanitized), custom test cases, and judge output with per-case results.
- A browsable **problem list** with filters, plus built-in collections (Blind 75, NeetCode 150 / 250 / All) and your own.

## Tech stack

| Layer | What's used |
| --- | --- |
| Shell | [Tauri 2](https://tauri.app/) (custom frameless title bar on Windows, overlay title bar on macOS) |
| Frontend | [SvelteKit](https://svelte.dev/docs/kit) + Svelte 5 runes, TypeScript, static adapter |
| Editor | [Monaco](https://microsoft.github.io/monaco-editor/) |
| Backend | Rust: `rusqlite` (bundled SQLite), `reqwest`, `tokio`, `keyring` |
| Storage | Two SQLite databases: a read-only problem **catalog** and a writable **user** DB (drafts, submissions, reviews, ratings, collections, chats) |

## Getting started

### Prerequisites

- **Node.js ≥ 22.13** (the catalog builder uses the built-in `node:sqlite`)
- **Rust** (stable) and the [Tauri 2 system prerequisites](https://tauri.app/start/prerequisites/) for your OS
- A LeetCode account, to run and submit code
- *(Optional)* an API key for Anthropic, OpenAI, or a compatible provider, to use Vin

### Run it

```bash
git clone <this-repo>
cd roostware-sommelier
npm install
npm run tauri dev
```

The first run builds the problem catalog automatically (`npm run catalog:ensure`): it downloads the [`whiskwhite/leetcode-complete`](https://huggingface.co/datasets/whiskwhite/leetcode-complete) dataset from Hugging Face and writes `src-tauri/resources/catalog.db`. After that, builds are instant and work offline.

### Build a release

```bash
npm run tauri build
```

### Useful scripts

| Command | What it does |
| --- | --- |
| `npm run tauri dev` | Launch the desktop app with hot reload |
| `npm run tauri build` | Produce installers for your platform |
| `npm run dev` | Frontend only, in the browser at `localhost:1420` (no Rust backend) |
| `npm run check` | Type-check the Svelte/TypeScript code |
| `npm run catalog` | Rebuild the problem catalog from the latest dataset |
| `npm run catalog -- --revision <sha>` | Pin the catalog to a specific dataset revision |
| `npm run test:rust` | Run the Rust test suite (builds the catalog first if missing) |

## Project layout

```
src/                    SvelteKit frontend
  routes/                 Dashboard (/), problem list (/problems), workspace (/solve)
  lib/                    Components, stores (*.svelte.ts) and IPC wrappers
src-tauri/              Rust backend
  src/recommend/          FSRS-6, Elo rating, pattern graph, recommendation engine
  src/coach/              Vin: providers, streaming (SSE), prompts, key storage
  src/leetcode/           Unofficial LeetCode run/submit client and sign-in
  src/db/                 SQLite access and numbered migrations
  data/                   Pattern map and curated problem lists (edit freely)
scripts/                catalog builder (build-catalog.mjs, catalog-schema.sql)
```

## Data and privacy

- Everything you create (drafts, submissions, ratings, review schedule, chats) is stored **locally** in SQLite.
- The catalog and your data are separate files, so refreshing the problem dataset never touches your progress.
- The only network traffic is to **LeetCode** (sign-in, run, submit) and to the **AI provider you configure**. Credentials are kept in the OS credential store.
- The dataset's AI-generated solutions are deliberately **not** imported.

## Status

Sommelier is an early, personal-scale project (`v0.1.0`).

- Windows and macOS are the supported platforms. Credential storage has no Linux backend yet.
- The LeetCode integration uses LeetCode's **unofficial** API, so it can break whenever LeetCode changes things.
- The AI coach's provider integrations are covered by tests against a local mock server, but haven't been exercised against the live provider APIs yet.

## Disclaimer

Sommelier is an independent project and is not affiliated with or endorsed by LeetCode, NeetCode, Anthropic, or OpenAI. Please use it in line with LeetCode's terms of service.

## License

MIT
