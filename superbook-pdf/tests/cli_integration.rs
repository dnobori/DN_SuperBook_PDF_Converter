//! CLI Integration Tests
//!
//! Tests for the CLI interface using assert_cmd

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn superbook_cmd() -> Command {
    Command::cargo_bin("superbook-pdf").unwrap()
}

#[test]
fn test_help_command() {
    superbook_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("superbook-pdf"))
        .stdout(predicate::str::contains("convert"))
        .stdout(predicate::str::contains("info"));
}

#[test]
fn test_version_command() {
    superbook_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_info_command() {
    superbook_cmd()
        .arg("info")
        .assert()
        .success()
        .stdout(predicate::str::contains("superbook-pdf"))
        .stdout(predicate::str::contains("System Information"))
        .stdout(predicate::str::contains("Platform"));
}

#[test]
fn test_convert_missing_input() {
    superbook_cmd()
        .args(["convert", "/nonexistent/path.pdf", "/tmp/out"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Input path does not exist"));
}

#[test]
fn test_convert_dry_run_single_file() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry Run"))
        .stdout(predicate::str::contains("Execution Plan"))
        .stdout(predicate::str::contains("Files to process: 1"));
}

#[test]
fn test_convert_dry_run_directory() {
    superbook_cmd()
        .args(["convert", "tests/fixtures", "/tmp/out", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry Run"))
        .stdout(predicate::str::contains("Files to process:"));
}

#[test]
fn test_convert_dry_run_with_options() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "--dpi",
            "600",
            "--deskew=true",
            "--margin-trim",
            "1.0",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("DPI: 600"))
        .stdout(predicate::str::contains("Deskew Correction: ENABLED"))
        .stdout(predicate::str::contains("Margin Trim: 1%"));
}

#[test]
fn test_convert_dry_run_gpu_options() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "--gpu=true",
            "--upscale=true",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("AI Upscaling"))
        .stdout(predicate::str::contains("GPU: YES"));
}

#[test]
fn test_convert_dry_run_thread_count() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "-t",
            "4",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Threads: 4"));
}

#[test]
fn test_convert_empty_directory() {
    let temp_dir = TempDir::new().unwrap();

    superbook_cmd()
        .args(["convert", temp_dir.path().to_str().unwrap(), "/tmp/out"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No PDF files found"));
}

// Requires external tools (ImageMagick, etc.)
#[test]
#[ignore = "requires external tool"]
fn test_convert_actual_pdf() {
    let temp_dir = TempDir::new().unwrap();
    let output_dir = temp_dir.path().join("output");

    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            output_dir.to_str().unwrap(),
            "-v",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Completed"));

    // Check output file exists
    let output_pdf = output_dir.join("sample_converted.pdf");
    assert!(output_pdf.exists());
}

// Test verbose output levels
#[test]
fn test_verbose_levels() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "-v",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Verbose: 1"));

    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "-vv",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Verbose: 2"));
}

#[test]
fn test_convert_help() {
    superbook_cmd()
        .args(["convert", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Convert PDF files"))
        .stdout(predicate::str::contains("INPUT"))
        .stdout(predicate::str::contains("OUTPUT"))
        .stdout(predicate::str::contains("--dpi"))
        .stdout(predicate::str::contains("--deskew"));
}

// Exit code tests
#[test]
fn test_exit_code_success_dry_run() {
    // Dry run with valid input should return success (exit code 0)
    superbook_cmd()
        .args(["convert", "tests/fixtures/sample.pdf", "/tmp/out", "--dry-run"])
        .assert()
        .code(0);
}

#[test]
fn test_exit_code_input_not_found() {
    // Nonexistent input should return exit code 3 (INPUT_NOT_FOUND)
    superbook_cmd()
        .args(["convert", "/nonexistent/path.pdf", "/tmp/out"])
        .assert()
        .code(3);
}

#[test]
fn test_exit_code_help_success() {
    // Help command should return success (exit code 0)
    superbook_cmd()
        .arg("--help")
        .assert()
        .code(0);
}

#[test]
fn test_exit_code_info_success() {
    // Info command should return success (exit code 0)
    superbook_cmd()
        .arg("info")
        .assert()
        .code(0);
}

#[test]
fn test_exit_code_no_pdfs_in_directory() {
    // Empty directory should return exit code 3 (INPUT_NOT_FOUND - no PDFs)
    let temp_dir = TempDir::new().unwrap();
    superbook_cmd()
        .args(["convert", temp_dir.path().to_str().unwrap(), "/tmp/out"])
        .assert()
        .code(3);
}

// Test OCR option display
#[test]
fn test_convert_dry_run_ocr_enabled() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "--ocr",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("OCR (YomiToku): ENABLED"));
}

// Test quiet mode (quiet mode affects runtime output, not dry-run output)
#[test]
fn test_convert_dry_run_quiet_mode() {
    // In dry-run mode, quiet flag doesn't change the plan output
    // Just verify the command succeeds with quiet flag
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "-q",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry Run"));
}

// Test no-upscale option
#[test]
fn test_convert_dry_run_no_upscale() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "--no-upscale",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("AI Upscaling: DISABLED"));
}

// Test no-deskew option
#[test]
fn test_convert_dry_run_no_deskew() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "--no-deskew",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deskew Correction: DISABLED"));
}

// Test no-gpu option
#[test]
fn test_convert_dry_run_no_gpu() {
    superbook_cmd()
        .args([
            "convert",
            "tests/fixtures/sample.pdf",
            "/tmp/out",
            "--dry-run",
            "--no-gpu",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("GPU: NO"));
}
