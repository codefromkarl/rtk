use crate::core::runner;
use crate::core::tracking;
use crate::core::utils::{exit_code_from_output, resolved_command, truncate};
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::ffi::OsString;

lazy_static! {
    static ref BANNER_RE: Regex = Regex::new(r"^Godot Engine v[\d.]+(?:\.[^\s]+)?").unwrap();
    static ref GPU_RE: Regex = Regex::new(r"^(?:OpenGL|Vulkan) API ").unwrap();
    static ref IMPORT_RE: Regex = Regex::new(r"^Importing (?:scene|resource): ").unwrap();
    static ref SHADER_RE: Regex = Regex::new(r"^Shader (?:compile|cache): ").unwrap();
    static ref TEXCOMP_RE: Regex = Regex::new(r"^Texture compress: ").unwrap();
    static ref ERROR_RE: Regex = Regex::new(r"^(?:ERROR|SCRIPT ERROR): ").unwrap();
    static ref WARNING_RE: Regex = Regex::new(r"^WARNING: ").unwrap();
    static ref AT_RE: Regex = Regex::new(r"^\s*At:\s+").unwrap();
    static ref EXPORT_OK_RE: Regex = Regex::new(r"^Export successful: ").unwrap();
    static ref EXPORT_FAIL_RE: Regex = Regex::new(r"^(?:ERROR: )?Export failed:").unwrap();
    static ref EXPORTING_RE: Regex = Regex::new(r"^Exporting project").unwrap();
    static ref GUT_RESULT_RE: Regex = Regex::new(r"^\s*(PASS|FAIL):\s+(.*)").unwrap();
    static ref GUT_SUMMARY_RE: Regex =
        Regex::new(r"^(\d+) tests?,\s+(\d+) passed?,\s+(\d+) failed?,\s+(\d+) pending?").unwrap();
    static ref GUT_TOTAL_RE: Regex = Regex::new(r"^--- Summary ---").unwrap();
    static ref GDUNIT_RESULT_RE: Regex =
        Regex::new(r"^(\s+)(\S+)\s+\.\.\.\s+(PASSED|FAILED)").unwrap();
    static ref GDUNIT_FAIL_DETAIL_RE: Regex = Regex::new(r"^\s{4}(res://.*:\d+: .*)").unwrap();
    static ref GDUNIT_SUMMARY_RE: Regex = Regex::new(r"^\s*Tests:\s+(\d+)").unwrap();
    static ref IMPORT_OK_RE: Regex = Regex::new(r"^Import completed").unwrap();
    static ref PARSE_ERROR_RE: Regex = Regex::new(r"^(?:SCRIPT ERROR|ERROR): (.+)").unwrap();
    static ref PARSE_LOCATION_RE: Regex = Regex::new(r"^\s*At: (res://\S+):(\d+)").unwrap();
}

pub fn run_export(args: &[String], verbose: u8) -> Result<i32> {
    run_filtered(args, verbose, "godot export", "godot_export", filter_export)
}

pub fn run_check(args: &[String], verbose: u8) -> Result<i32> {
    run_filtered(args, verbose, "godot check", "godot_check", filter_check)
}

pub fn run_test(args: &[String], verbose: u8) -> Result<i32> {
    run_filtered(args, verbose, "godot test", "godot_test", filter_test)
}

pub fn run_script(args: &[String], verbose: u8) -> Result<i32> {
    run_filtered(args, verbose, "godot script", "godot_script", filter_script)
}

pub fn run_import(args: &[String], verbose: u8) -> Result<i32> {
    run_filtered(args, verbose, "godot import", "godot_import", filter_import)
}

fn run_filtered<F>(
    args: &[String],
    verbose: u8,
    tool_name: &str,
    tee_label: &'static str,
    filter_fn: F,
) -> Result<i32>
where
    F: Fn(&str) -> String,
{
    let mut cmd = resolved_command("godot");
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: godot {}", args.join(" "));
    }

    runner::run_filtered(
        cmd,
        tool_name,
        &args.join(" "),
        filter_fn,
        runner::RunOptions::with_tee(tee_label),
    )
}

pub fn run_other(args: &[OsString], verbose: u8) -> Result<i32> {
    if args.is_empty() {
        anyhow::bail!("godot: no subcommand specified");
    }

    let timer = tracking::TimedExecution::start();
    let subcommand = args[0].to_string_lossy();
    let mut cmd = resolved_command("godot");
    cmd.arg(&*subcommand);
    for arg in &args[1..] {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: godot {} ...", subcommand);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run godot {}", subcommand))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    print!("{}", stdout);
    eprint!("{}", stderr);

    timer.track(
        &format!("godot {}", subcommand),
        &format!("rtk godot {}", subcommand),
        &raw,
        &raw,
    );

    Ok(exit_code_from_output(&output, "godot"))
}

fn filter_export(output: &str) -> String {
    let mut import_count = 0usize;
    let mut shader_count = 0usize;
    let mut texture_count = 0usize;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut export_result: Option<String> = None;
    let mut lines = output.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if BANNER_RE.is_match(trimmed)
            || GPU_RE.is_match(trimmed)
            || trimmed.is_empty()
            || EXPORTING_RE.is_match(trimmed)
        {
            continue;
        }
        if IMPORT_RE.is_match(trimmed) {
            import_count += 1;
            continue;
        }
        if SHADER_RE.is_match(trimmed) {
            shader_count += 1;
            continue;
        }
        if TEXCOMP_RE.is_match(trimmed) {
            texture_count += 1;
            continue;
        }
        if ERROR_RE.is_match(trimmed) {
            errors.push(with_at_line(trimmed, &mut lines));
            continue;
        }
        if WARNING_RE.is_match(trimmed) {
            warnings.push(with_at_line(trimmed, &mut lines));
            continue;
        }
        if AT_RE.is_match(trimmed) {
            continue;
        }
        if EXPORT_OK_RE.is_match(trimmed) || EXPORT_FAIL_RE.is_match(trimmed) {
            export_result = Some(trimmed.to_string());
        }
    }

    if let Some(result) = export_result {
        let mut out = format!("godot export: {}", result);
        append_export_counts(&mut out, import_count, shader_count, texture_count);
        append_issue_block(&mut out, "errors", &errors, 5);
        append_issue_count(&mut out, "warnings", warnings.len());
        out
    } else if !errors.is_empty() {
        let mut out = format!("godot export: {} errors", errors.len());
        for e in errors.iter().take(10) {
            out.push_str(&format!("\n  {}", truncate(e, 120)));
        }
        if errors.len() > 10 {
            out.push_str(&format!("\n  ... +{} more", errors.len() - 10));
        }
        out
    } else {
        "godot export: completed (no output)".to_string()
    }
}

fn filter_check(output: &str) -> String {
    let mut errors_by_file: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut warning_count = 0usize;
    let mut lines = output.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if BANNER_RE.is_match(trimmed) || GPU_RE.is_match(trimmed) || trimmed.is_empty() {
            continue;
        }
        if let Some(caps) = PARSE_ERROR_RE.captures(trimmed) {
            let msg = caps[1].to_string();
            if let Some(next) = lines.peek() {
                if let Some(loc) = PARSE_LOCATION_RE.captures(next.trim()) {
                    errors_by_file
                        .entry(loc[1].to_string())
                        .or_default()
                        .push((loc[2].to_string(), msg));
                    lines.next();
                    continue;
                }
            }
            errors_by_file
                .entry("(unknown)".to_string())
                .or_default()
                .push(("0".to_string(), msg));
            continue;
        }
        if WARNING_RE.is_match(trimmed) {
            warning_count += 1;
            if let Some(next) = lines.peek() {
                if AT_RE.is_match(next.trim()) {
                    lines.next();
                }
            }
        }
    }

    let total_errors: usize = errors_by_file.values().map(Vec::len).sum();
    if total_errors == 0 && warning_count == 0 {
        return "godot check: no issues found".to_string();
    }
    if total_errors == 0 {
        return format!("godot check: {} warnings", warning_count);
    }

    let mut result = format!(
        "godot check: {} errors in {} files",
        total_errors,
        errors_by_file.len()
    );
    if warning_count > 0 {
        result.push_str(&format!(", {} warnings", warning_count));
    }

    let mut files: Vec<&str> = errors_by_file.keys().map(|s| s.as_str()).collect();
    files.sort();
    for file in files {
        let errors = &errors_by_file[file];
        let mut line_nums: Vec<&str> = errors.iter().map(|(line, _)| line.as_str()).collect();
        line_nums.sort();
        result.push_str(&format!("\n  {}: {} errors", file, errors.len()));
        if !line_nums.is_empty() {
            result.push_str(&format!(
                " (lines {})",
                line_nums
                    .iter()
                    .take(6)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if errors.len() > 6 {
            result.push_str(&format!(" +{} more", errors.len() - 6));
        }
    }
    result
}

fn filter_test(output: &str) -> String {
    if output.contains("GdUnit4 Runner") || output.contains("Running test suite:") {
        filter_gdunit_test(output)
    } else {
        filter_gut_test(output)
    }
}

fn filter_gut_test(output: &str) -> String {
    let mut total_tests = 0usize;
    let mut total_passed = 0usize;
    let mut total_failed = 0usize;
    let mut failures: Vec<(String, String, String)> = Vec::new();
    let mut current_script = String::new();
    let mut expect_total = false;
    let mut got_total = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Running script: ") {
            current_script = rest.to_string();
            continue;
        }
        if let Some(caps) = GUT_RESULT_RE.captures(trimmed) {
            if &caps[1] == "FAIL" {
                let detail = caps[2].to_string();
                failures.push((
                    current_script.clone(),
                    detail.split(" - ").next().unwrap_or(&detail).to_string(),
                    detail,
                ));
            }
            continue;
        }
        if GUT_TOTAL_RE.is_match(trimmed) {
            expect_total = true;
            continue;
        }
        if let Some(caps) = GUT_SUMMARY_RE.captures(trimmed) {
            let tests: usize = caps[1].parse().unwrap_or(0);
            let passed: usize = caps[2].parse().unwrap_or(0);
            let failed: usize = caps[3].parse().unwrap_or(0);
            if expect_total {
                total_tests = tests;
                total_passed = passed;
                total_failed = failed;
                got_total = true;
                expect_total = false;
            } else if !got_total {
                total_tests += tests;
                total_passed += passed;
                total_failed += failed;
            }
        }
    }

    format_test_summary(total_tests, total_passed, total_failed, failures, true)
}

fn filter_gdunit_test(output: &str) -> String {
    let mut total_tests = 0usize;
    let mut total_passed = 0usize;
    let mut total_failed = 0usize;
    let mut failures: Vec<(String, String, String)> = Vec::new();
    let mut current_suite = String::new();
    let mut last_test_name = String::new();
    let mut last_failed = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Running test suite: ") {
            current_suite = rest.to_string();
            continue;
        }
        if let Some(caps) = GDUNIT_RESULT_RE.captures(line) {
            last_test_name = caps[2].to_string();
            last_failed = &caps[3] == "FAILED";
            if !last_failed {
                total_passed += 1;
            }
            continue;
        }
        if let Some(caps) = GDUNIT_FAIL_DETAIL_RE.captures(line) {
            if last_failed {
                failures.push((
                    current_suite.clone(),
                    last_test_name.clone(),
                    caps[1].to_string(),
                ));
            }
            continue;
        }
        if let Some(caps) = GDUNIT_SUMMARY_RE.captures(trimmed) {
            total_tests = caps[1].parse().unwrap_or(0);
            continue;
        }
        if let Some(v) = trimmed.strip_prefix("Passed:") {
            total_passed = v.trim().parse().unwrap_or(total_passed);
            continue;
        }
        if let Some(v) = trimmed.strip_prefix("Failed:") {
            total_failed = v.trim().parse().unwrap_or(total_failed);
        }
    }
    if total_failed == 0 {
        total_failed = failures.len();
    }
    if total_tests == 0 {
        total_tests = total_passed + total_failed;
    }

    format_test_summary(total_tests, total_passed, total_failed, failures, false)
}

fn format_test_summary(
    total_tests: usize,
    total_passed: usize,
    total_failed: usize,
    failures: Vec<(String, String, String)>,
    include_script_name: bool,
) -> String {
    if total_tests == 0 && failures.is_empty() {
        return "godot test: no tests found".to_string();
    }
    if total_failed == 0 {
        return format!(
            "godot test: {} passed ({} tests)",
            total_passed, total_tests
        );
    }

    let mut result = format!(
        "godot test: {} passed, {} failed ({} tests)",
        total_passed, total_failed, total_tests
    );
    for (suite_or_script, test, detail) in failures {
        result.push_str(&format!("\n\n  FAIL: {}", test));
        if include_script_name && !suite_or_script.is_empty() {
            result.push_str(&format!(
                "\n    {}",
                suite_or_script
                    .rsplit('/')
                    .next()
                    .unwrap_or(&suite_or_script)
            ));
        }
        result.push_str(&format!("\n    {}", truncate(&detail, 100)));
    }
    result
}

fn filter_script(output: &str) -> String {
    let filtered: Vec<&str> = output
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !BANNER_RE.is_match(trimmed)
                && !GPU_RE.is_match(trimmed)
                && !trimmed.is_empty()
                && !trimmed.starts_with("Scene tree initialized.")
                && !trimmed.starts_with("Loading resources")
                && !trimmed.starts_with("Done loading.")
                && !trimmed.starts_with("DEBUG:")
                && !trimmed.starts_with("Script execution completed")
        })
        .collect();
    if filtered.is_empty() {
        "godot script: no output".to_string()
    } else {
        filtered.join("\n")
    }
}

fn filter_import(output: &str) -> String {
    let mut import_count = 0usize;
    let mut shader_count = 0usize;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut import_ok = false;
    let mut lines = output.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if BANNER_RE.is_match(trimmed) || GPU_RE.is_match(trimmed) || trimmed.is_empty() {
            continue;
        }
        if IMPORT_RE.is_match(trimmed) {
            import_count += 1;
            continue;
        }
        if SHADER_RE.is_match(trimmed) {
            shader_count += 1;
            continue;
        }
        if ERROR_RE.is_match(trimmed) {
            errors.push(with_at_line(trimmed, &mut lines));
            continue;
        }
        if WARNING_RE.is_match(trimmed) {
            warnings.push(with_at_line(trimmed, &mut lines));
            continue;
        }
        if IMPORT_OK_RE.is_match(trimmed) {
            import_ok = true;
        }
    }

    let mut result = if import_ok {
        "godot import: ok".to_string()
    } else if !errors.is_empty() {
        format!("godot import: {} errors", errors.len())
    } else {
        "godot import: completed".to_string()
    };
    if import_count > 0 || shader_count > 0 {
        result.push_str(&format!(
            "\n  {}",
            [
                (import_count > 0).then(|| format!("{} resources imported", import_count)),
                (shader_count > 0).then(|| format!("{} shaders processed", shader_count)),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ")
        ));
    }
    append_issue_block(&mut result, "errors", &errors, 5);
    append_issue_count(&mut result, "warnings", warnings.len());
    result
}

fn with_at_line<'a, I>(line: &str, lines: &mut std::iter::Peekable<I>) -> String
where
    I: Iterator<Item = &'a str>,
{
    let mut out = line.to_string();
    if let Some(next) = lines.peek() {
        let trimmed = next.trim();
        if AT_RE.is_match(trimmed) {
            out.push_str(&format!("\n  {}", truncate(trimmed, 120)));
            lines.next();
        }
    }
    out
}

fn append_export_counts(out: &mut String, imports: usize, shaders: usize, textures: usize) {
    let parts = [
        (imports > 0).then(|| format!("{} resources imported", imports)),
        (shaders > 0).then(|| format!("{} shaders compiled", shaders)),
        (textures > 0).then(|| format!("{} textures compressed", textures)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if !parts.is_empty() {
        out.push_str(&format!("\n  {}", parts.join(", ")));
    }
}

fn append_issue_block(out: &mut String, label: &str, issues: &[String], limit: usize) {
    if issues.is_empty() {
        return;
    }
    out.push_str(&format!("\n  {} {}", issues.len(), label));
    for issue in issues.iter().take(limit) {
        out.push_str(&format!("\n    {}", truncate(issue, 120)));
    }
    if issues.len() > limit {
        out.push_str(&format!("\n    ... +{} more", issues.len() - limit));
    }
}

fn append_issue_count(out: &mut String, label: &str, count: usize) {
    if count > 0 {
        out.push_str(&format!("\n  {} {}", count, label));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_export_success() {
        let input = include_str!("../../../tests/fixtures/godot_export_raw.txt");
        let output = filter_export(input);
        assert!(output.contains("godot export:"));
        assert!(output.contains("build/game.x86_64"));
        assert!(output.contains("resources imported"));
        assert!(output.contains("shaders compiled"));
        assert!(output.contains("textures compressed"));
        assert!(!output.contains("Importing scene"));
    }

    #[test]
    fn test_filter_export_failure() {
        let input = include_str!("../../../tests/fixtures/godot_export_fail_raw.txt");
        let output = filter_export(input);
        assert!(output.contains("Shader compilation failed"));
        assert!(output.contains("Export failed"));
    }

    #[test]
    fn test_filter_check_errors() {
        let input = include_str!("../../../tests/fixtures/godot_check_raw.txt");
        let output = filter_check(input);
        assert!(output.contains("res://scripts/player.gd"));
        assert!(output.contains("res://scripts/enemy.gd"));
        assert!(output.contains("res://utils/helpers.gd"));
        assert!(output.contains("warnings"));
    }

    #[test]
    fn test_filter_gut_test_with_failures() {
        let input = include_str!("../../../tests/fixtures/godot_gut_test_raw.txt");
        let output = filter_test(input);
        assert!(output.contains("12 passed"));
        assert!(output.contains("3 failed"));
        assert!(output.contains("FAIL: test_death"));
        assert!(output.contains("FAIL: test_spawn"));
        assert!(output.contains("FAIL: test_damage"));
        assert!(!output.contains("PASS:"));
    }

    #[test]
    fn test_filter_gdunit_test_with_failures() {
        let input = include_str!("../../../tests/fixtures/godot_gdunit_test_raw.txt");
        let output = filter_test(input);
        assert!(output.contains("8 passed"));
        assert!(output.contains("3 failed"));
        assert!(output.contains("test_death"));
        assert!(output.contains("test_spawn"));
        assert!(output.contains("test_damage"));
    }

    #[test]
    fn test_filter_script() {
        let input = include_str!("../../../tests/fixtures/godot_script_raw.txt");
        let output = filter_script(input);
        assert!(output.contains("Player stats:"));
        assert!(output.contains("Inventory:"));
        assert!(!output.contains("Godot Engine"));
    }

    #[test]
    fn test_filter_import() {
        let input = include_str!("../../../tests/fixtures/godot_import_raw.txt");
        let output = filter_import(input);
        assert!(output.contains("godot import: ok"));
        assert!(output.contains("resources imported"));
        assert!(output.contains("shaders processed"));
        assert!(output.contains("1 warnings"));
    }

    #[test]
    fn test_token_savings_thresholds() {
        let cases = [
            (
                include_str!("../../../tests/fixtures/godot_export_raw.txt"),
                filter_export as fn(&str) -> String,
                60.0,
            ),
            (
                include_str!("../../../tests/fixtures/godot_check_raw.txt"),
                filter_check as fn(&str) -> String,
                50.0,
            ),
            (
                include_str!("../../../tests/fixtures/godot_gut_test_raw.txt"),
                filter_test as fn(&str) -> String,
                60.0,
            ),
            (
                include_str!("../../../tests/fixtures/godot_gdunit_test_raw.txt"),
                filter_test as fn(&str) -> String,
                60.0,
            ),
            (
                include_str!("../../../tests/fixtures/godot_script_raw.txt"),
                filter_script as fn(&str) -> String,
                40.0,
            ),
            (
                include_str!("../../../tests/fixtures/godot_import_raw.txt"),
                filter_import as fn(&str) -> String,
                60.0,
            ),
        ];
        for (input, filter, min) in cases {
            let output = filter(input);
            let savings =
                100.0 - (count_tokens(&output) as f64 / count_tokens(input) as f64 * 100.0);
            assert!(
                savings >= min,
                "expected >={min}% savings, got {savings:.1}%"
            );
        }
    }
}
