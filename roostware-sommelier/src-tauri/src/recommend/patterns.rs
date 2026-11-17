//! The patterns the app tracks, and which problems belong to which.
//!
//! The definitions live in `data/patterns.json` and `data/curated.json` (compiled in), so they
//! can be reviewed and edited without touching code or the catalog. A problem's patterns come
//! from its LeetCode topic tags:
//!
//!   * each tag maps to at most one pattern (see the JSON); tags like "Sorting" that say nothing
//!     about the technique are ignored;
//!   * BFS/DFS tags count toward Graphs only when the problem isn't also a tree problem;
//!   * a hand-picked list of interval problems counts toward Sorting & Intervals (LeetCode has
//!     no such tag);
//!   * tier-0 patterns (Arrays & Hashing, Math & Bits) are fallbacks, credited only when no more
//!     specific pattern applies, because nearly every problem carries "Array".

use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

use serde::Deserialize;

const PATTERNS_JSON: &str = include_str!("../../data/patterns.json");
const CURATED_JSON: &str = include_str!("../../data/curated.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPattern {
    id: String,
    name: String,
    short: String,
    tier: u8,
    requires: Vec<String>,
    tags: Vec<String>,
    #[serde(default)]
    tags_unless_tree: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPatterns {
    tree_tags: Vec<String>,
    patterns: Vec<RawPattern>,
    interval_problems: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawCurated {
    blind75: Vec<String>,
    neetcode150: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub id: String,
    pub name: String,
    /// A label short enough for a chart axis.
    pub short: String,
    pub tier: u8,
    /// Indices of the patterns that should be covered (partly solved) before this one is recommended
    /// at anything but its easiest.
    pub requires: Vec<usize>,
}

/// Which curated lists a problem is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Curated {
    #[default]
    None,
    /// NeetCode 150 only.
    NeetCode150,
    /// Blind 75 (all of which are also in NeetCode 150).
    Blind75,
}

impl Curated {
    pub fn label(self) -> Option<&'static str> {
        match self {
            Curated::None => None,
            Curated::NeetCode150 => Some("NeetCode 150"),
            Curated::Blind75 => Some("Blind 75"),
        }
    }
}

pub struct PatternSet {
    pub patterns: Vec<Pattern>,
    tag_owner: HashMap<String, usize>,
    unless_tree: HashMap<String, usize>,
    tree_tags: HashSet<String>,
    interval_slugs: HashSet<String>,
    interval_pattern: usize,
    blind75: HashSet<String>,
    neetcode150: HashSet<String>,
}

impl PatternSet {
    fn load() -> PatternSet {
        let raw: RawPatterns = serde_json::from_str(PATTERNS_JSON).expect("data/patterns.json is valid");
        let curated: RawCurated = serde_json::from_str(CURATED_JSON).expect("data/curated.json is valid");

        let index: HashMap<&str, usize> = raw.patterns.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
        let patterns = raw
            .patterns
            .iter()
            .map(|p| Pattern {
                id: p.id.clone(),
                name: p.name.clone(),
                short: p.short.clone(),
                tier: p.tier,
                requires: p.requires.iter().map(|r| *index.get(r.as_str()).unwrap_or_else(|| panic!("unknown pattern {r}"))).collect(),
            })
            .collect();

        let mut tag_owner = HashMap::new();
        let mut unless_tree = HashMap::new();
        for (i, p) in raw.patterns.iter().enumerate() {
            for t in &p.tags {
                assert!(tag_owner.insert(t.clone(), i).is_none(), "tag {t} is mapped twice");
            }
            for t in &p.tags_unless_tree {
                assert!(unless_tree.insert(t.clone(), i).is_none(), "tag {t} is mapped twice");
            }
        }
        PatternSet {
            patterns,
            tag_owner,
            unless_tree,
            tree_tags: raw.tree_tags.into_iter().collect(),
            interval_slugs: raw.interval_problems.into_iter().collect(),
            interval_pattern: *index.get("sorting-intervals").expect("a sorting-intervals pattern"),
            blind75: curated.blind75.into_iter().collect(),
            neetcode150: curated.neetcode150.into_iter().collect(),
        }
    }

    /// The patterns a problem counts toward (sorted, no repeats; empty when its tags say nothing
    /// and it isn't a listed interval problem).
    pub fn classify(&self, tags: &[String], slug: &str) -> Vec<usize> {
        let is_tree = tags.iter().any(|t| self.tree_tags.contains(t));
        let mut found: Vec<usize> = Vec::new();
        for t in tags {
            if let Some(&p) = self.tag_owner.get(t) {
                found.push(p);
            } else if let (Some(&p), false) = (self.unless_tree.get(t), is_tree) {
                found.push(p);
            }
        }
        if self.interval_slugs.contains(slug) {
            found.push(self.interval_pattern);
        }
        // Fallback patterns only when nothing more specific applies.
        if found.iter().any(|&p| self.patterns[p].tier >= 1) {
            found.retain(|&p| self.patterns[p].tier >= 1);
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    pub fn curated(&self, slug: &str) -> Curated {
        if self.blind75.contains(slug) {
            Curated::Blind75
        } else if self.neetcode150.contains(slug) {
            Curated::NeetCode150
        } else {
            Curated::None
        }
    }

    /// All curated slugs (for checking them against the catalog).
    pub fn curated_slugs(&self) -> impl Iterator<Item = &String> {
        self.blind75.iter().chain(self.neetcode150.iter())
    }

    pub fn interval_slugs(&self) -> impl Iterator<Item = &String> {
        self.interval_slugs.iter()
    }

    /// Every tag the definitions mention.
    pub fn mapped_tags(&self) -> impl Iterator<Item = &String> {
        self.tag_owner.keys().chain(self.unless_tree.keys())
    }
}

/// The definitions, parsed once.
pub fn definitions() -> &'static PatternSet {
    static SET: OnceLock<PatternSet> = OnceLock::new();
    SET.get_or_init(PatternSet::load)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn names(set: &PatternSet, found: Vec<usize>) -> Vec<&str> {
        found.into_iter().map(|i| set.patterns[i].id.as_str()).collect()
    }

    #[test]
    fn there_are_seventeen_patterns_with_sane_prerequisites() {
        let set = definitions();
        assert_eq!(set.patterns.len(), 17);
        for (i, p) in set.patterns.iter().enumerate() {
            for &r in &p.requires {
                assert!(set.patterns[r].tier <= p.tier, "{} requires {}, which comes later", p.id, set.patterns[r].id);
                assert_ne!(r, i);
            }
            if p.tier == 0 {
                assert!(p.requires.is_empty(), "foundation patterns are always open");
            }
            // No cycles: following the requirements never leads back to the pattern itself, so
            // every pattern can eventually be unlocked.
            let mut stack: Vec<usize> = p.requires.clone();
            let mut seen = HashSet::new();
            while let Some(r) = stack.pop() {
                assert_ne!(r, i, "{} (indirectly) requires itself", p.id);
                if seen.insert(r) {
                    stack.extend(&set.patterns[r].requires);
                }
            }
        }
        let ids: HashSet<_> = set.patterns.iter().map(|p| &p.id).collect();
        assert_eq!(ids.len(), 17, "ids are unique");
    }

    #[test]
    fn a_specific_pattern_beats_the_generic_array_tag() {
        let set = definitions();
        assert_eq!(names(set, set.classify(&tags(&["Array", "Hash Table"]), "two-sum")), ["arrays-hashing"]);
        assert_eq!(names(set, set.classify(&tags(&["Array", "Two Pointers", "Sorting"]), "x")), ["two-pointers"]);
        assert_eq!(
            names(set, set.classify(&tags(&["Array", "Binary Search", "Dynamic Programming"]), "x")),
            ["binary-search", "dynamic-programming"],
            "a problem can belong to several specific patterns"
        );
        assert_eq!(names(set, set.classify(&tags(&["Math", "Array"]), "x")), ["arrays-hashing", "math-bits"], "both fallbacks when nothing else");
    }

    #[test]
    fn sorting_and_unknown_tags_say_nothing() {
        let set = definitions();
        assert!(set.classify(&tags(&["Sorting"]), "x").is_empty());
        assert!(set.classify(&tags(&["Interactive"]), "x").is_empty());
        assert!(set.classify(&[], "x").is_empty(), "untagged problems have no pattern");
    }

    #[test]
    fn bfs_and_dfs_count_as_graphs_only_off_trees() {
        let set = definitions();
        assert_eq!(names(set, set.classify(&tags(&["Depth-First Search", "Matrix", "Array"]), "number-of-islands")), ["graphs"]);
        assert_eq!(
            names(set, set.classify(&tags(&["Tree", "Depth-First Search", "Binary Tree"]), "maximum-depth-of-binary-tree")),
            ["trees"],
            "a tree traversal is a Trees problem, not Graphs"
        );
        assert_eq!(
            names(set, set.classify(&tags(&["Graph", "Tree", "Breadth-First Search"]), "x")),
            ["trees", "graphs"],
            "an explicit Graph tag still counts"
        );
    }

    #[test]
    fn listed_interval_problems_count_as_intervals() {
        let set = definitions();
        assert_eq!(names(set, set.classify(&tags(&["Array", "Sorting"]), "merge-intervals")), ["sorting-intervals"]);
        assert_eq!(
            names(set, set.classify(&tags(&["Array", "Sorting"]), "k-closest-points")),
            ["arrays-hashing"],
            "any other Array + Sorting problem is just an arrays problem"
        );
        assert_eq!(
            names(set, set.classify(&[], "merge-intervals")),
            ["sorting-intervals"],
            "even if the catalog has no tags for it"
        );
    }

    #[test]
    fn curated_lists_are_sized_right_and_nested() {
        let set = definitions();
        assert_eq!(set.blind75.len(), 75);
        assert_eq!(set.neetcode150.len(), 150);
        assert_eq!(set.curated("two-sum"), Curated::Blind75);
        assert_eq!(set.curated("trapping-rain-water"), Curated::NeetCode150);
        assert_eq!(set.curated("not-a-real-problem"), Curated::None);
        let outside: Vec<_> = set.blind75.iter().filter(|s| !set.neetcode150.contains(*s)).collect();
        // Blind 75 and NeetCode 150 overlap almost completely; a few Blind 75 entries (e.g. the
        // Combination Sum variant) aren't in NeetCode's.
        assert!(outside.len() <= 6, "Blind 75 problems missing from NeetCode 150: {outside:?}");
    }

    /// Checks the data files against the real catalog, when it has been built: every slug must
    /// exist, every mapped tag must be a real tag, and the pool sizes must look right.
    #[test]
    fn data_files_match_the_real_catalog() {
        let Some((_dir, db)) = crate::db::test_support::open_real_catalog() else {
            eprintln!("skipping: catalog.db not built");
            return;
        };
        let set = definitions();
        db.with(|c| {
            let slugs: HashSet<String> = c
                .prepare("SELECT title_slug FROM catalog.problems")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let missing: Vec<_> = set.curated_slugs().chain(set.interval_slugs()).filter(|s| !slugs.contains(*s)).collect();
            assert!(missing.is_empty(), "slugs not in the catalog: {missing:?}");

            let catalog_tags: HashSet<String> = c
                .prepare("SELECT name FROM catalog.tags")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let unknown: Vec<_> = set.mapped_tags().filter(|t| !catalog_tags.contains(*t)).collect();
            assert!(unknown.is_empty(), "mapped tags that aren't in the catalog: {unknown:?}");
            let unmapped: Vec<_> = catalog_tags.iter().filter(|t| !set.mapped_tags().any(|m| m == *t)).collect();
            assert!(unmapped.len() <= 2, "new catalog tags nobody mapped: {unmapped:?} (Sorting and Interactive are ignored on purpose)");
            Ok(())
        })
        .unwrap();
    }
}
