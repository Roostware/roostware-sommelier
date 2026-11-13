//! Turns LeetCode's (unofficial, undocumented) result JSON into our own types.
//!
//! Parsing is deliberately forgiving: every field is optional, numbers may arrive as strings
//! and strings as numbers, and anything unexpected simply becomes `None`. The raw response is
//! kept alongside (`result_json` for submissions), so a format change never loses data.

use serde::Serialize;
use serde_json::Value;

use crate::db::submissions::NewSubmission;

// ---- Run ------------------------------------------------------------------------------------

/// One example test case from a Run, with what the code produced for it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCase {
    /// The case's input: one line per argument.
    pub input: String,
    pub expected: String,
    pub output: String,
    /// What the code printed while running this case.
    pub stdout: String,
    pub passed: bool,
}

/// The outcome of running code against test cases without submitting it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    /// "Accepted" (every case matched), "Wrong Answer", or LeetCode's own message when the code
    /// didn't run to completion ("Compile Error", "Runtime Error", "Time Limit Exceeded", …).
    pub status: String,
    /// Whether every case matched. False whenever the code didn't run to completion.
    pub correct: bool,
    pub cases: Vec<RunCase>,
    /// When the code didn't run to completion, the one case LeetCode says it stopped on (a line
    /// per argument), if it says. Never the whole input that was sent: with several cases that
    /// would claim all of them were executed. `None` when the run completed.
    pub last_input: Option<String>,
    pub runtime_ms: Option<i64>,
    pub memory_kb: Option<i64>,
    /// Compile or runtime error text.
    pub error: Option<String>,
}

pub fn parse_run(v: &Value, input: &str) -> RunResult {
    let message = text(v, "status_msg");
    let ran = v["run_success"].as_bool().unwrap_or(false);
    let error = error_text(v);

    // How many cases there are: LeetCode's per-case pass/fail string ("101") is the most direct
    // signal, then its test count. (The answer lists can carry an extra trailing "" entry, so
    // their length alone isn't trusted.)
    let compare = text(v, "compare_result").filter(|c| c.chars().all(|ch| ch == '0' || ch == '1'));
    let answers = strings(v, "code_answer");
    let expected = strings(v, "expected_code_answer");
    let count = if !ran {
        0
    } else if let Some(c) = &compare {
        c.len()
    } else {
        match int(v, "total_testcases") {
            Some(n) if n >= 0 && (n as usize) <= expected.len() => n as usize,
            _ => expected.len(),
        }
    };

    // The input holds one line per argument for every case, so it splits into equal chunks.
    let lines: Vec<&str> = input.lines().collect();
    let per_case = if count > 0 && lines.len() % count == 0 { lines.len() / count } else { 0 };
    let stdouts = strings(v, "std_output_list");

    let cases: Vec<RunCase> = (0..count)
        .map(|i| {
            let output = answers.get(i).cloned().unwrap_or_default();
            let want = expected.get(i).cloned().unwrap_or_default();
            let passed = match &compare {
                Some(c) => c.as_bytes().get(i) == Some(&b'1'),
                None => output == want,
            };
            RunCase {
                input: if per_case > 0 { lines[i * per_case..(i + 1) * per_case].join("\n") } else if i == 0 { input.to_owned() } else { String::new() },
                expected: want,
                output,
                stdout: stdouts.get(i).cloned().unwrap_or_default(),
                passed,
            }
        })
        .collect();

    let correct = ran && v["correct_answer"].as_bool().unwrap_or_else(|| !cases.is_empty() && cases.iter().all(|c| c.passed));
    let status = if !ran {
        message.unwrap_or_else(|| "Error".into())
    } else if correct {
        "Accepted".into()
    } else {
        "Wrong Answer".into()
    };

    // Only meaningful when the run stopped early; a completed run reports every case itself.
    let last_input = if ran { None } else { text(v, "last_testcase") };

    RunResult { status, correct, cases, last_input, runtime_ms: runtime_ms(v), memory_kb: memory_kb(v), error }
}

// ---- Submit ---------------------------------------------------------------------------------

/// Builds the record for a finished submission from LeetCode's final response.
pub fn parse_submission(
    v: &Value,
    problem_id: i64,
    lang: &str,
    code: &str,
    leetcode_submission_id: Option<String>,
) -> NewSubmission {
    NewSubmission {
        problem_id,
        lang: lang.to_owned(),
        code: code.to_owned(),
        leetcode_submission_id,
        status: text(v, "status_msg").unwrap_or_else(|| "Unknown".into()),
        status_code: int(v, "status_code"),
        runtime_ms: runtime_ms(v),
        runtime_percentile: float(v, "runtime_percentile"),
        memory_kb: memory_kb(v),
        memory_percentile: float(v, "memory_percentile"),
        total_correct: int(v, "total_correct"),
        total_testcases: int(v, "total_testcases"),
        last_testcase: text(v, "last_testcase"),
        expected_output: text(v, "expected_output"),
        code_output: text(v, "code_output"),
        error_message: error_text(v),
        result_json: Some(v.to_string()),
    }
}

// ---- Field helpers --------------------------------------------------------------------------

/// A non-empty string field (numbers are accepted and stringified).
fn text(v: &Value, key: &str) -> Option<String> {
    match &v[key] {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .map(|a| a.iter().map(|x| x.as_str().map(str::to_owned).unwrap_or_else(|| x.to_string())).collect())
        .unwrap_or_default()
}

fn int(v: &Value, key: &str) -> Option<i64> {
    match &v[key] {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn float(v: &Value, key: &str) -> Option<f64> {
    match &v[key] {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// "1 ms" -> 1. "N/A" (what LeetCode sends for failures) -> None.
fn runtime_ms(v: &Value) -> Option<i64> {
    let s = text(v, "status_runtime")?;
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Prefers LeetCode's byte count; falls back to the display text ("20.4 MB").
fn memory_kb(v: &Value) -> Option<i64> {
    if let Some(bytes) = int(v, "memory").filter(|b| *b > 0) {
        return Some(bytes / 1024);
    }
    let s = text(v, "status_memory")?;
    let number: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let value: f64 = number.parse().ok()?;
    let unit = s[number.len()..].trim().to_ascii_uppercase();
    let factor = match unit.as_str() {
        "KB" => 1.0,
        "MB" => 1024.0,
        "GB" => 1024.0 * 1024.0,
        _ => return None,
    };
    Some((value * factor).round() as i64)
}

/// The compile or runtime error, preferring the full text over the short one.
fn error_text(v: &Value) -> Option<String> {
    ["full_compile_error", "compile_error", "full_runtime_error", "runtime_error"]
        .iter()
        .find_map(|k| text(v, k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Captured from the real API (Two Sum, Python 3).
    fn accepted_submission() -> Value {
        json!({
            "status_code": 10, "lang": "python3", "run_success": true, "status_runtime": "1 ms",
            "memory": 20844000, "status_memory": "20.4 MB", "runtime_percentile": 64.4589,
            "memory_percentile": 51.6892, "total_correct": 66, "total_testcases": 66,
            "status_msg": "Accepted", "state": "SUCCESS", "last_testcase": "", "expected_output": "",
            "code_output": ""
        })
    }

    fn wrong_submission() -> Value {
        json!({
            "status_code": 11, "lang": "python3", "run_success": true, "status_runtime": "N/A",
            "status_memory": "N/A", "total_correct": 0, "total_testcases": 66,
            "status_msg": "Wrong Answer", "state": "SUCCESS", "last_testcase": "[2,7,11,15]\n9",
            "expected_output": "[0,1]", "code_output": "[0,0]"
        })
    }

    #[test]
    fn accepted_submission_maps_every_field() {
        let s = parse_submission(&accepted_submission(), 1, "python3", "code", Some("2194979879".into()));
        assert_eq!(s.status, "Accepted");
        assert_eq!(s.status_code, Some(10));
        assert_eq!(s.runtime_ms, Some(1));
        assert_eq!(s.memory_kb, Some(20844000 / 1024));
        assert_eq!(s.runtime_percentile, Some(64.4589));
        assert_eq!(s.memory_percentile, Some(51.6892));
        assert_eq!((s.total_correct, s.total_testcases), (Some(66), Some(66)));
        assert_eq!(s.leetcode_submission_id.as_deref(), Some("2194979879"));
        // Empty strings from LeetCode mean "nothing", not an empty failure.
        assert_eq!((s.last_testcase, s.expected_output, s.code_output, s.error_message), (None, None, None, None));
        assert!(s.result_json.unwrap().contains("\"status_msg\":\"Accepted\""));
    }

    #[test]
    fn wrong_answer_carries_the_failing_case_and_no_runtime() {
        let s = parse_submission(&wrong_submission(), 1, "python3", "code", None);
        assert_eq!(s.status, "Wrong Answer");
        assert_eq!((s.runtime_ms, s.memory_kb, s.runtime_percentile, s.memory_percentile), (None, None, None, None));
        assert_eq!(s.total_correct, Some(0));
        assert_eq!(s.last_testcase.as_deref(), Some("[2,7,11,15]\n9"));
        assert_eq!(s.expected_output.as_deref(), Some("[0,1]"));
        assert_eq!(s.code_output.as_deref(), Some("[0,0]"));
    }

    #[test]
    fn compile_and_runtime_errors_become_the_error_message() {
        let compile = json!({"status_msg": "Compile Error", "status_code": 20, "run_success": false,
            "compile_error": "short", "full_compile_error": "line 3: invalid syntax"});
        assert_eq!(parse_submission(&compile, 1, "python3", "c", None).error_message.as_deref(), Some("line 3: invalid syntax"));
        let runtime = json!({"status_msg": "Runtime Error", "status_code": 15, "runtime_error": "IndexError"});
        let s = parse_submission(&runtime, 1, "python3", "c", None);
        assert_eq!((s.status.as_str(), s.error_message.as_deref()), ("Runtime Error", Some("IndexError")));
    }

    #[test]
    fn unexpected_shapes_degrade_to_none_instead_of_failing() {
        let s = parse_submission(&json!({"runtime_percentile": "12.5", "total_correct": "3", "memory": null}), 1, "cpp", "c", None);
        assert_eq!(s.status, "Unknown");
        assert_eq!((s.runtime_percentile, s.total_correct, s.memory_kb), (Some(12.5), Some(3), None));
        assert_eq!(parse_submission(&json!(null), 1, "cpp", "c", None).status, "Unknown");
    }

    #[test]
    fn memory_falls_back_to_the_display_text() {
        let kb = |t: &str| memory_kb(&json!({"status_memory": t}));
        assert_eq!(kb("20.4 MB"), Some(20890));
        assert_eq!(kb("512 KB"), Some(512));
        assert_eq!(kb("N/A"), None);
        assert_eq!(kb("12 parsecs"), None);
    }

    #[test]
    fn runtime_parses_leading_digits() {
        let ms = |t: &str| runtime_ms(&json!({"status_runtime": t}));
        assert_eq!((ms("0 ms"), ms("123 ms"), ms("N/A"), ms("")), (Some(0), Some(123), None, None));
    }

    // The single-case Run captured from the real API (note the trailing "" in code_answer).
    #[test]
    fn single_case_run_from_the_real_api() {
        let v = json!({"status_msg": "Accepted", "run_success": true, "lang": "python3",
            "code_answer": ["[0,1]", ""], "expected_code_answer": ["[0,1]", ""], "correct_answer": true,
            "status_runtime": "0 ms", "status_memory": "19 MB", "total_correct": 1, "total_testcases": 1});
        let r = parse_run(&v, "[2,7,11,15]\n9");
        assert_eq!(r.status, "Accepted");
        assert!(r.correct);
        assert_eq!(r.runtime_ms, Some(0));
        assert_eq!(r.memory_kb, Some(19 * 1024));
        assert_eq!(r.cases.len(), 1, "the trailing \"\" is not a second case");
        assert_eq!(r.cases[0].output, "[0,1]");
        // The same holds when compare_result is what says how many cases there are.
        let with_compare = parse_run(&json!({"status_msg": "Accepted", "run_success": true, "compare_result": "1",
            "code_answer": ["[0,1]", ""], "expected_code_answer": ["[0,1]", ""], "correct_answer": true}), "[2,7,11,15]\n9");
        assert_eq!(with_compare.cases.len(), 1);
        assert_eq!(with_compare.cases[0].input, "[2,7,11,15]\n9");
        assert_eq!(with_compare.cases[0].output, "[0,1]");
    }

    #[test]
    fn multi_case_run_splits_the_input_and_marks_each_case() {
        let v = json!({"status_msg": "Accepted", "run_success": true, "compare_result": "101",
            "code_answer": ["[0,1]", "[0,0]", "[0,1]"], "expected_code_answer": ["[0,1]", "[1,2]", "[0,1]"],
            "std_output_list": ["", "debug\n", ""], "correct_answer": false});
        let r = parse_run(&v, "[2,7,11,15]\n9\n[3,2,4]\n6\n[3,3]\n6");
        assert_eq!(r.status, "Wrong Answer", "LeetCode says Accepted for any run that completes");
        assert!(!r.correct);
        assert_eq!(r.cases.len(), 3);
        assert_eq!(r.cases[1].input, "[3,2,4]\n6");
        assert_eq!((r.cases[1].output.as_str(), r.cases[1].expected.as_str()), ("[0,0]", "[1,2]"));
        assert_eq!(r.cases.iter().map(|c| c.passed).collect::<Vec<_>>(), [true, false, true]);
        assert_eq!(r.cases[1].stdout, "debug\n");
    }

    #[test]
    fn run_that_does_not_complete_reports_leetcodes_message_and_error() {
        let v = json!({"status_msg": "Compile Error", "run_success": false, "full_compile_error": "bad syntax"});
        let r = parse_run(&v, "1\n2");
        assert_eq!((r.status.as_str(), r.correct, r.cases.len()), ("Compile Error", false, 0));
        assert_eq!(r.last_input, None, "LeetCode didn't say which case it stopped on, so none is claimed");
        assert_eq!(r.error.as_deref(), Some("bad syntax"));

        let tle = parse_run(&json!({"status_msg": "Time Limit Exceeded", "run_success": false}), "1");
        assert_eq!(tle.status, "Time Limit Exceeded");
    }

    #[test]
    fn the_case_a_run_stopped_on_is_reported_only_if_leetcode_says_and_only_that_case() {
        // Three cases were sent; LeetCode names the one that crashed.
        let crashed = json!({"status_msg": "Runtime Error", "run_success": false, "runtime_error": "IndexError",
            "last_testcase": "[3,2,4]\n6"});
        let r = parse_run(&crashed, "[2,7,11,15]\n9\n[3,2,4]\n6\n[3,3]\n6");
        assert_eq!(r.last_input.as_deref(), Some("[3,2,4]\n6"), "just that case, not all three");

        // A run that finished has per-case results, so a stray last_testcase isn't repeated.
        let finished = json!({"status_msg": "Accepted", "run_success": true, "compare_result": "1",
            "code_answer": ["1"], "expected_code_answer": ["1"], "last_testcase": "1"});
        assert_eq!(parse_run(&finished, "1").last_input, None);

        // An empty value means "none".
        let blank = json!({"status_msg": "Runtime Error", "run_success": false, "last_testcase": ""});
        assert_eq!(parse_run(&blank, "1").last_input, None);
    }

    #[test]
    fn run_without_compare_result_falls_back_to_comparing_outputs() {
        let v = json!({"status_msg": "Accepted", "run_success": true,
            "code_answer": ["1", "2"], "expected_code_answer": ["1", "3"]});
        let r = parse_run(&v, "a\nb");
        assert_eq!(r.cases.iter().map(|c| c.passed).collect::<Vec<_>>(), [true, false]);
        assert!(!r.correct);
    }
}
