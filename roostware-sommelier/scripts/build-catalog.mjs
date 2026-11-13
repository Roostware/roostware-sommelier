#!/usr/bin/env node
// Builds catalog.db — the read-only LeetCode problem catalog — from the
// whiskwhite/leetcode-complete HuggingFace dataset.
//
//   npm run catalog                      # latest revision -> src-tauri/resources/catalog.db
//   npm run catalog -- --revision <sha>  # pin a dataset revision
//   npm run catalog -- --out <path>      # write somewhere else
//   npm run catalog:ensure               # build only if missing/unusable; no network otherwise
//
// `catalog:ensure` (--if-missing) is what `tauri dev` and `tauri build` run first. A catalog
// that is present, readable and on the current schema version is left alone, so ordinary
// builds are instant and work offline. Run plain `npm run catalog` to refresh the dataset.
//
// Requires Node >= 22.13 (built-in `node:sqlite`); no npm dependencies.
//
// The dataset's AI-generated `solutions` are intentionally NOT imported.
// catalog.db holds only problem data; everything user-specific (drafts,
// submissions, collections, notes) lives in user.db and references problems by
// `problems.id`, so rebuilding the catalog never touches user data.

import { DatabaseSync } from "node:sqlite";
import { createReadStream, createWriteStream, readFileSync } from "node:fs";
import { mkdir, rename, rm, stat } from "node:fs/promises";
import { createInterface } from "node:readline";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const DATASET = "whiskwhite/leetcode-complete";
const SPLITS = ["train", "validation", "test", "unsolved"];
const SCHEMA_VERSION = 1;
const KEEP_CATEGORY = "Algorithms"; // dataset `category` value to import; the rest are skipped

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const { values: args } = parseArgs({
  options: {
    out: { type: "string", default: path.join(root, "src-tauri", "resources", "catalog.db") },
    cache: { type: "string", default: path.join(root, "scripts", ".cache") },
    revision: { type: "string", default: "main" },
    "if-missing": { type: "boolean", default: false },
  },
});

// Why the file at `file` can't be used as-is, or null if it's a valid current catalog.
async function unusableReason(file) {
  if (!(await stat(file).then(() => true, () => false))) return "not built yet";
  let db;
  try {
    db = new DatabaseSync(file, { readOnly: true });
    const version = db.prepare("SELECT value FROM meta WHERE key = 'schema_version'").get()?.value;
    if (version !== String(SCHEMA_VERSION)) {
      return `schema version is ${version ?? "missing"}, expected ${SCHEMA_VERSION}`;
    }
    if (db.prepare("SELECT COUNT(*) AS n FROM problems").get().n === 0) return "it has no problems";
    return null;
  } catch (e) {
    return `unreadable (${e.message})`;
  } finally {
    db?.close();
  }
}

// ---------------------------------------------------------------- download --

async function resolveRevision(revision) {
  const res = await fetch(`https://huggingface.co/api/datasets/${DATASET}/revision/${revision}`);
  if (!res.ok) throw new Error(`Could not resolve revision "${revision}": HTTP ${res.status}`);
  const { sha } = await res.json();
  if (!sha) throw new Error("HuggingFace API response had no sha");
  return sha;
}

async function download(sha, split) {
  const dir = path.join(args.cache, sha);
  const file = path.join(dir, `${split}.jsonl`);
  if (await stat(file).then(() => true, () => false)) {
    console.log(`  ${split}.jsonl cached`);
    return file;
  }
  await mkdir(dir, { recursive: true });
  const url = `https://huggingface.co/datasets/${DATASET}/resolve/${sha}/${split}.jsonl`;
  console.log(`  ${split}.jsonl downloading…`);
  const res = await fetch(url);
  if (!res.ok || !res.body) throw new Error(`Download failed for ${url}: HTTP ${res.status}`);
  // Write to a temp name first so an interrupted download never looks cached.
  const tmp = `${file}.part`;
  await pipeline(Readable.fromWeb(res.body), createWriteStream(tmp));
  await rename(tmp, file);
  return file;
}

async function* readJsonl(file) {
  const rl = createInterface({ input: createReadStream(file, "utf8"), crlfDelay: Infinity });
  let n = 0;
  for await (const line of rl) {
    n++;
    if (!line.trim()) continue;
    try {
      yield JSON.parse(line);
    } catch (e) {
      throw new Error(`${path.basename(file)} line ${n}: invalid JSON (${e.message})`);
    }
  }
}

// ------------------------------------------------------------------ schema --

const SCHEMA = readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "catalog-schema.sql"), "utf8");

// ------------------------------------------------------------------- build --

function toInt(value, field, rec) {
  const n = Number(value);
  if (!Number.isInteger(n)) {
    throw new Error(`problem ${rec.title_slug ?? rec.id}: ${field} is not an integer: ${JSON.stringify(value)}`);
  }
  return n;
}

function requireString(value, field, rec) {
  if (typeof value !== "string") {
    throw new Error(`problem ${rec.title_slug ?? rec.id}: ${field} is not a string`);
  }
  return value;
}

async function main() {
  if (args["if-missing"]) {
    const reason = await unusableReason(args.out);
    if (reason === null) {
      console.log(`catalog.db is present and current; skipping (run \`npm run catalog\` to refresh it)`);
      return;
    }
    console.log(`Building catalog.db: ${reason}`);
  }

  console.log(`Resolving ${DATASET}@${args.revision}`);
  const sha = await resolveRevision(args.revision);
  console.log(`Revision ${sha}`);

  const files = [];
  for (const split of SPLITS) files.push([split, await download(sha, split)]);

  await mkdir(path.dirname(args.out), { recursive: true });
  const tmpOut = `${args.out}.building`;
  await rm(tmpOut, { force: true });

  const db = new DatabaseSync(tmpOut);
  db.exec("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF; PRAGMA foreign_keys = ON;");
  db.exec(SCHEMA);

  const insProblem = db.prepare(`
    INSERT INTO problems (id, frontend_id, title, title_slug, difficulty, content_html,
      example_test_cases, is_paid_only, likes, dislikes, total_accepted, total_submissions,
      acceptance_rate, created_at_approx)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`);
  const insTag = db.prepare("INSERT OR IGNORE INTO tags (name) VALUES (?)");
  const getTag = db.prepare("SELECT id FROM tags WHERE name = ?");
  const insProblemTag = db.prepare("INSERT OR IGNORE INTO problem_tags (problem_id, tag_id) VALUES (?, ?)");
  const insSnippet = db.prepare("INSERT OR REPLACE INTO code_snippets (problem_id, lang, code) VALUES (?, ?, ?)");
  const insFts = db.prepare("INSERT INTO problems_fts (rowid, title, frontend_id, tags) VALUES (?, ?, ?, ?)");

  // Load everything first so duplicates can be resolved. `solutions` is
  // dropped here, as each record is read, and never held or imported.
  //
  // The same id can appear twice (e.g. 627 as both "Swap Salary" and its
  // renamed "Swap Sex of Employees"): the dataset holds a stale and a fresh
  // snapshot. Keep the one with more total_submissions, i.e. the newer one,
  // since its slug is the one LeetCode currently serves.
  const byId = new Map();
  let duplicates = 0;
  const skipped = new Map(); // category -> count, for non-algorithm problems
  for (const [split, file] of files) {
    let count = 0;
    for await (const { solutions: _dropped, ...rec } of readJsonl(file)) {
      // Only algorithm problems are kept (not Database, Shell, Concurrency, …).
      if (rec.category !== KEEP_CATEGORY) {
        skipped.set(rec.category, (skipped.get(rec.category) ?? 0) + 1);
        continue;
      }
      const id = toInt(rec.id, "id", rec);
      const prev = byId.get(id);
      if (prev) {
        duplicates++;
        const keep = (rec.total_submissions ?? 0) > (prev.total_submissions ?? 0) ? rec : prev;
        const drop = keep === rec ? prev : rec;
        console.warn(`  duplicate id ${id}: keeping "${keep.title_slug}", dropping "${drop.title_slug}"`);
        byId.set(id, keep);
      } else {
        byId.set(id, rec);
      }
      count++;
    }
    console.log(`  ${split}: ${count} records`);
  }
  const records = [...byId.values()].sort((a, b) => Number(a.frontend_id) - Number(b.frontend_id));
  const seen = new Set(byId.keys());

  db.exec("BEGIN");
  try {
    for (const rec of records) {
      const id = toInt(rec.id, "id", rec);
      const frontendId = toInt(rec.frontend_id, "frontend_id", rec);
      const tags = Array.isArray(rec.topic_tags) ? [...new Set(rec.topic_tags)] : [];

      insProblem.run(
        id,
        frontendId,
        requireString(rec.title, "title", rec),
        requireString(rec.title_slug, "title_slug", rec),
        requireString(rec.difficulty, "difficulty", rec),
        requireString(rec.content, "content", rec),
        rec.example_test_cases ?? "",
        rec.is_paid_only ? 1 : 0,
        rec.likes ?? 0,
        rec.dislikes ?? 0,
        rec.total_accepted ?? 0,
        rec.total_submissions ?? 0,
        rec.acceptance_rate ?? 0,
        rec.created_at_approx ?? null,
      );

      for (const name of tags) {
        insTag.run(name);
        insProblemTag.run(id, getTag.get(name).id);
      }
      for (const snip of rec.code_snippets ?? []) {
        insSnippet.run(id, snip.lang, snip.code);
      }
      insFts.run(id, rec.title, String(frontendId), tags.join(" "));
    }

    const meta = db.prepare("INSERT INTO meta (key, value) VALUES (?, ?)");
    meta.run("schema_version", String(SCHEMA_VERSION));
    meta.run("dataset", DATASET);
    meta.run("dataset_revision", sha);
    meta.run("built_at", new Date().toISOString());
    meta.run("problem_count", String(seen.size));
    db.exec(`PRAGMA user_version = ${SCHEMA_VERSION}`);
    db.exec("COMMIT");
  } catch (e) {
    db.exec("ROLLBACK");
    db.close();
    await rm(tmpOut, { force: true });
    throw e;
  }

  // Sanity checks before the new file replaces the old one.
  const fk = db.prepare("PRAGMA foreign_key_check").all();
  if (fk.length) throw new Error(`foreign_key_check found ${fk.length} violations`);
  const integrity = db.prepare("PRAGMA integrity_check").get();
  if (integrity.integrity_check !== "ok") throw new Error(`integrity_check: ${integrity.integrity_check}`);
  const n = (sql) => db.prepare(sql).get().n;
  const summary = {
    problems: n("SELECT COUNT(*) AS n FROM problems"),
    paid: n("SELECT COUNT(*) AS n FROM problems WHERE is_paid_only = 1"),
    tags: n("SELECT COUNT(*) AS n FROM tags"),
    problem_tags: n("SELECT COUNT(*) AS n FROM problem_tags"),
    code_snippets: n("SELECT COUNT(*) AS n FROM code_snippets"),
    fts_rows: n("SELECT COUNT(*) AS n FROM problems_fts"),
  };
  if (summary.problems === 0) throw new Error("No problems imported");
  if (summary.fts_rows !== summary.problems) throw new Error("FTS row count does not match problems");

  db.exec("VACUUM");
  db.exec("PRAGMA optimize");
  db.close();
  await rm(args.out, { force: true });
  await rename(tmpOut, args.out);

  const { size } = await stat(args.out);
  console.log(`\nWrote ${path.relative(root, args.out)} (${(size / 1024 / 1024).toFixed(1)} MB)`);
  console.table(summary);
  if (skipped.size) {
    console.log(`Skipped non-${KEEP_CATEGORY} problems: ${[...skipped].map(([c, n]) => `${c} ${n}`).join(", ")}`);
  }
  if (duplicates) console.warn(`Resolved ${duplicates} duplicate problem id(s)`);
}

main().catch((e) => {
  console.error(e.stack ?? e);
  process.exit(1);
});
