//! CLI interface module
//!
//! Provides command-line interface using clap derive macros.

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

/// Exit codes for the CLI
///
/// These codes follow standard Unix conventions and provide
/// specific error categories for scripting and automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// 正常終了
    Success = 0,
    /// 一般的なエラー
    GeneralError = 1,
    /// 引数エラー
    InvalidArgs = 2,
    /// 入力ファイル/ディレクトリが見つからない
    InputNotFound = 3,
    /// 出力エラー（書き込み権限など）
    OutputError = 4,
    /// 処理中のエラー
    ProcessingError = 5,
    /// GPU初期化/処理エラー
    GpuError = 6,
    /// 外部ツール（Python等）エラー
    ExternalToolError = 7,
}

impl ExitCode {
    /// Convert to process exit code
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Get human-readable description
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            ExitCode::Success => "Success",
            ExitCode::GeneralError => "General error",
            ExitCode::InvalidArgs => "Invalid arguments",
            ExitCode::InputNotFound => "Input file or directory not found",
            ExitCode::OutputError => "Output error (permission denied, disk full, etc.)",
            ExitCode::ProcessingError => "Processing error",
            ExitCode::GpuError => "GPU initialization or processing error",
            ExitCode::ExternalToolError => "External tool error (Python, ImageMagick, etc.)",
        }
    }
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code.code()
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        std::process::ExitCode::from(code.code() as u8)
    }
}

/// High-quality PDF converter for scanned books
#[derive(Parser, Debug)]
#[command(name = "superbook-pdf")]
#[command(author = "DN_SuperBook_PDF_Converter Contributors")]
#[command(version)]
#[command(about = "High-quality PDF converter for scanned books", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Convert PDF files with AI enhancement
    Convert(ConvertArgs),
    /// Show system information
    Info,
}

/// Arguments for the convert command
#[derive(clap::Args, Debug)]
pub struct ConvertArgs {
    /// Input PDF file or directory
    pub input: PathBuf,

    /// Output directory
    #[arg(default_value = "./output")]
    pub output: PathBuf,

    /// Enable Japanese OCR (YomiToku)
    #[arg(short, long)]
    pub ocr: bool,

    /// Enable AI upscaling (RealESRGAN)
    #[arg(short, long, default_value_t = true)]
    #[arg(action = clap::ArgAction::Set)]
    pub upscale: bool,

    /// Disable AI upscaling
    #[arg(long = "no-upscale")]
    #[arg(action = clap::ArgAction::SetTrue)]
    no_upscale: bool,

    /// Enable deskew correction
    #[arg(short, long, default_value_t = true)]
    #[arg(action = clap::ArgAction::Set)]
    pub deskew: bool,

    /// Disable deskew correction
    #[arg(long = "no-deskew")]
    #[arg(action = clap::ArgAction::SetTrue)]
    no_deskew: bool,

    /// Margin trim percentage
    #[arg(short, long, default_value_t = 0.5)]
    pub margin_trim: f32,

    /// Output DPI
    #[arg(long, default_value_t = 300)]
    pub dpi: u32,

    /// Number of parallel threads
    #[arg(short, long)]
    pub threads: Option<usize>,

    /// Enable GPU processing
    #[arg(short, long, default_value_t = true)]
    #[arg(action = clap::ArgAction::Set)]
    pub gpu: bool,

    /// Disable GPU processing
    #[arg(long = "no-gpu")]
    #[arg(action = clap::ArgAction::SetTrue)]
    no_gpu: bool,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress progress output
    #[arg(short, long)]
    pub quiet: bool,

    /// Show execution plan without processing
    #[arg(long)]
    pub dry_run: bool,
}

impl ConvertArgs {
    /// Get effective upscale setting (considering --no-upscale flag)
    pub fn effective_upscale(&self) -> bool {
        self.upscale && !self.no_upscale
    }

    /// Get effective deskew setting (considering --no-deskew flag)
    pub fn effective_deskew(&self) -> bool {
        self.deskew && !self.no_deskew
    }

    /// Get effective GPU setting (considering --no-gpu flag)
    pub fn effective_gpu(&self) -> bool {
        self.gpu && !self.no_gpu
    }

    /// Get thread count (default to available CPUs)
    pub fn thread_count(&self) -> usize {
        self.threads.unwrap_or_else(num_cpus::get)
    }
}

/// Create a styled progress bar for file processing
pub fn create_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )
            .expect("Invalid progress bar template")
            .progress_chars("#>-"),
    );
    pb
}

/// Create a spinner for indeterminate progress
pub fn create_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Invalid spinner template"),
    );
    pb.set_message(message.to_string());
    pb
}

/// Create a progress bar for page processing
pub fn create_page_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] Page {pos}/{len} ({percent}%) - {msg}")
            .expect("Invalid progress bar template")
            .progress_chars("█▓░"),
    );
    pb
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_parse() {
        // Verify CLI can be built
        Cli::command().debug_assert();
    }

    #[test]
    fn test_help_display() {
        let mut cmd = Cli::command();
        let help = cmd.render_help().to_string();
        assert!(help.contains("superbook-pdf"));
        assert!(help.contains("convert"));
    }

    #[test]
    fn test_version_display() {
        let cmd = Cli::command();
        let version = cmd.get_version().unwrap_or("unknown");
        assert!(!version.is_empty());
    }

    #[test]
    fn test_missing_input_error() {
        let result = Cli::try_parse_from(["superbook-pdf", "convert"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("required"));
    }

    #[test]
    fn test_option_parsing() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--ocr",
            "--no-upscale",
            "--dpi",
            "600",
            "-vvv",
        ])
        .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert!(args.ocr);
            assert!(!args.effective_upscale());
            assert_eq!(args.dpi, 600);
            assert_eq!(args.verbose, 3);
        } else {
            panic!("Expected Convert command");
        }
    }

    #[test]
    fn test_default_values() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();

        if let Commands::Convert(args) = cli.command {
            assert!(!args.ocr);
            assert!(args.effective_upscale());
            assert!(args.effective_deskew());
            assert_eq!(args.margin_trim, 0.5);
            assert_eq!(args.dpi, 300);
            assert!(args.effective_gpu());
            assert_eq!(args.verbose, 0);
            assert!(!args.quiet);
            assert!(!args.dry_run);
        } else {
            panic!("Expected Convert command");
        }
    }

    #[test]
    fn test_directory_input() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "/tmp/test_dir", "--dry-run"])
            .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert!(args.dry_run);
        } else {
            panic!("Expected Convert command");
        }
    }

    #[test]
    fn test_info_command() {
        let cli = Cli::try_parse_from(["superbook-pdf", "info"]).unwrap();

        assert!(matches!(cli.command, Commands::Info));
    }

    // TC-CLI-007: Progress bar display
    #[test]
    fn test_progress_bar_display() {
        // Test that progress bar can be created and styled
        let pb = create_progress_bar(100);
        assert_eq!(pb.length(), Some(100));

        // Test progress updates
        pb.set_position(50);
        assert_eq!(pb.position(), 50);

        pb.finish_with_message("done");
    }

    #[test]
    fn test_spinner_creation() {
        let spinner = create_spinner("Processing...");
        assert_eq!(spinner.message(), "Processing...");
        spinner.finish_with_message("Complete");
    }

    #[test]
    fn test_page_progress_bar() {
        let pb = create_page_progress_bar(10);
        assert_eq!(pb.length(), Some(10));

        for i in 0..10 {
            pb.set_position(i);
            pb.set_message(format!("page_{}.png", i));
        }
        pb.finish_with_message("All pages processed");
    }

    // Exit code tests
    #[test]
    fn test_exit_code_values() {
        assert_eq!(ExitCode::Success.code(), 0);
        assert_eq!(ExitCode::GeneralError.code(), 1);
        assert_eq!(ExitCode::InvalidArgs.code(), 2);
        assert_eq!(ExitCode::InputNotFound.code(), 3);
        assert_eq!(ExitCode::OutputError.code(), 4);
        assert_eq!(ExitCode::ProcessingError.code(), 5);
        assert_eq!(ExitCode::GpuError.code(), 6);
        assert_eq!(ExitCode::ExternalToolError.code(), 7);
    }

    #[test]
    fn test_exit_code_descriptions() {
        assert_eq!(ExitCode::Success.description(), "Success");
        assert!(!ExitCode::GeneralError.description().is_empty());
        assert!(!ExitCode::InvalidArgs.description().is_empty());
        assert!(!ExitCode::InputNotFound.description().is_empty());
        assert!(!ExitCode::OutputError.description().is_empty());
        assert!(!ExitCode::ProcessingError.description().is_empty());
        assert!(!ExitCode::GpuError.description().is_empty());
        assert!(!ExitCode::ExternalToolError.description().is_empty());
    }

    #[test]
    fn test_exit_code_into_i32() {
        let code: i32 = ExitCode::Success.into();
        assert_eq!(code, 0);

        let code: i32 = ExitCode::ExternalToolError.into();
        assert_eq!(code, 7);
    }

    #[test]
    fn test_exit_code_equality() {
        assert_eq!(ExitCode::Success, ExitCode::Success);
        assert_ne!(ExitCode::Success, ExitCode::GeneralError);
    }

    #[test]
    fn test_exit_code_clone_copy() {
        let code = ExitCode::ProcessingError;
        let cloned = code.clone();
        let copied = code;
        assert_eq!(code, cloned);
        assert_eq!(code, copied);
    }

    // Additional CLI tests

    #[test]
    fn test_thread_count_default() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();

        if let Commands::Convert(args) = cli.command {
            // Should default to available CPUs
            assert!(args.thread_count() > 0);
        } else {
            panic!("Expected Convert command");
        }
    }

    #[test]
    fn test_thread_count_explicit() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--threads", "4"])
            .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.thread_count(), 4);
        } else {
            panic!("Expected Convert command");
        }
    }

    #[test]
    fn test_all_flags_combination() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--ocr",
            "--no-upscale",
            "--no-deskew",
            "--no-gpu",
            "--quiet",
            "--dry-run",
            "-vvv",
        ])
        .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert!(args.ocr);
            assert!(!args.effective_upscale());
            assert!(!args.effective_deskew());
            assert!(!args.effective_gpu());
            assert!(args.quiet);
            assert!(args.dry_run);
            assert_eq!(args.verbose, 3);
        } else {
            panic!("Expected Convert command");
        }
    }

    #[test]
    fn test_margin_trim_setting() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--margin-trim",
            "1.5",
        ])
        .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.margin_trim, 1.5);
        } else {
            panic!("Expected Convert command");
        }
    }

    #[test]
    fn test_dpi_setting() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--dpi", "600"]).unwrap();

        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.dpi, 600);
        } else {
            panic!("Expected Convert command");
        }
    }

    #[test]
    fn test_exit_code_to_process_exit_code() {
        let code: std::process::ExitCode = ExitCode::Success.into();
        // ExitCode doesn't expose its value, but we can verify conversion works
        let _ = code;

        let code: std::process::ExitCode = ExitCode::GeneralError.into();
        let _ = code;
    }

    // Test all exit codes
    #[test]
    fn test_all_exit_codes() {
        let codes = [
            (ExitCode::Success, 0),
            (ExitCode::GeneralError, 1),
            (ExitCode::InvalidArgs, 2),
            (ExitCode::InputNotFound, 3),
            (ExitCode::OutputError, 4),
            (ExitCode::ProcessingError, 5),
            (ExitCode::GpuError, 6),
            (ExitCode::ExternalToolError, 7),
        ];

        for (exit_code, expected) in codes {
            assert_eq!(exit_code.code(), expected);
        }
    }

    // Test version command
    #[test]
    fn test_version_flag() {
        // --version should trigger version output (handled by clap)
        let result = Cli::try_parse_from(["superbook-pdf", "--version"]);
        // clap returns an error for --version (it's a special flag)
        assert!(result.is_err());
    }

    // Test help flag
    #[test]
    fn test_help_flag() {
        let result = Cli::try_parse_from(["superbook-pdf", "--help"]);
        // clap returns an error for --help (it's a special flag)
        assert!(result.is_err());
    }

    // Test invalid command
    #[test]
    fn test_invalid_command() {
        let result = Cli::try_parse_from(["superbook-pdf", "invalid-command"]);
        assert!(result.is_err());
    }

    // Test missing required argument
    #[test]
    fn test_missing_input_file() {
        let result = Cli::try_parse_from(["superbook-pdf", "convert"]);
        assert!(result.is_err());
    }

    // Test negative DPI (invalid)
    #[test]
    fn test_invalid_dpi() {
        let result =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--dpi", "-100"]);
        // Should fail parsing or validation
        assert!(result.is_err());
    }

    // Test zero threads (explicitly set)
    #[test]
    fn test_zero_threads() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--threads", "0"])
            .unwrap();

        if let Commands::Convert(args) = cli.command {
            // threads=0 is explicitly set, returned as-is
            let count = args.thread_count();
            // When Some(0) is provided, it returns 0
            assert_eq!(count, 0);
        } else {
            panic!("Expected Convert command");
        }
    }

    // Test very high thread count
    #[test]
    fn test_high_thread_count() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--threads", "1024"])
                .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.thread_count(), 1024);
        } else {
            panic!("Expected Convert command");
        }
    }

    // Test output path argument (positional)
    #[test]
    fn test_output_path() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "/custom/output"])
            .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.output, PathBuf::from("/custom/output"));
        } else {
            panic!("Expected Convert command");
        }
    }

    // Test default output path
    #[test]
    fn test_default_output_path() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();

        if let Commands::Convert(args) = cli.command {
            // Default output is "./output"
            assert_eq!(args.output, PathBuf::from("./output"));
        } else {
            panic!("Expected Convert command");
        }
    }

    // Test margin trim boundary values
    #[test]
    fn test_margin_trim_boundaries() {
        // Zero margin
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--margin-trim",
            "0.0",
        ])
        .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.margin_trim, 0.0);
        } else {
            panic!("Expected Convert command");
        }

        // Large margin
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--margin-trim",
            "10.0",
        ])
        .unwrap();

        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.margin_trim, 10.0);
        } else {
            panic!("Expected Convert command");
        }
    }

    // Test verbosity levels
    #[test]
    fn test_verbosity_levels() {
        // No verbosity
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.verbose, 0);
        }

        // -v
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-v"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.verbose, 1);
        }

        // -vv
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-vv"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.verbose, 2);
        }
    }

    // Test quiet flag
    #[test]
    fn test_quiet_flag() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--quiet"]).unwrap();

        if let Commands::Convert(args) = cli.command {
            assert!(args.quiet);
        } else {
            panic!("Expected Convert command");
        }
    }

    // Test dry-run flag
    #[test]
    fn test_dry_run_flag() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--dry-run"]).unwrap();

        if let Commands::Convert(args) = cli.command {
            assert!(args.dry_run);
        } else {
            panic!("Expected Convert command");
        }
    }

    // ============ Debug Implementation Tests ============

    #[test]
    fn test_exit_code_debug_impl() {
        let code = ExitCode::Success;
        let debug_str = format!("{:?}", code);
        assert!(debug_str.contains("Success"));

        let code2 = ExitCode::ProcessingError;
        let debug_str2 = format!("{:?}", code2);
        assert!(debug_str2.contains("ProcessingError"));
    }

    #[test]
    fn test_cli_debug_impl() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        let debug_str = format!("{:?}", cli);
        assert!(debug_str.contains("Cli"));
        assert!(debug_str.contains("command"));
    }

    #[test]
    fn test_commands_debug_impl() {
        let cli = Cli::try_parse_from(["superbook-pdf", "info"]).unwrap();
        let debug_str = format!("{:?}", cli.command);
        assert!(debug_str.contains("Info"));

        let cli2 = Cli::try_parse_from(["superbook-pdf", "convert", "test.pdf"]).unwrap();
        let debug_str2 = format!("{:?}", cli2.command);
        assert!(debug_str2.contains("Convert"));
    }

    #[test]
    fn test_convert_args_debug_impl() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--ocr"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            let debug_str = format!("{:?}", args);
            assert!(debug_str.contains("ConvertArgs"));
            assert!(debug_str.contains("input"));
            assert!(debug_str.contains("ocr"));
        }
    }

    // ============ Clone/Copy Tests ============

    #[test]
    fn test_exit_code_copy() {
        let original = ExitCode::GpuError;
        let copied: ExitCode = original; // Copy
        let _still_valid = original; // original still valid due to Copy
        assert_eq!(copied, ExitCode::GpuError);
    }

    #[test]
    fn test_exit_code_clone() {
        let original = ExitCode::OutputError;
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    // ============ ExitCode Additional Tests ============

    #[test]
    fn test_all_exit_code_descriptions_non_empty() {
        let codes = [
            ExitCode::Success,
            ExitCode::GeneralError,
            ExitCode::InvalidArgs,
            ExitCode::InputNotFound,
            ExitCode::OutputError,
            ExitCode::ProcessingError,
            ExitCode::GpuError,
            ExitCode::ExternalToolError,
        ];

        for code in codes {
            let desc = code.description();
            assert!(!desc.is_empty(), "Description for {:?} is empty", code);
        }
    }

    #[test]
    fn test_exit_code_unique_values() {
        let codes = [
            ExitCode::Success,
            ExitCode::GeneralError,
            ExitCode::InvalidArgs,
            ExitCode::InputNotFound,
            ExitCode::OutputError,
            ExitCode::ProcessingError,
            ExitCode::GpuError,
            ExitCode::ExternalToolError,
        ];

        // All codes should have unique values
        let values: Vec<i32> = codes.iter().map(|c| c.code()).collect();
        for (i, v1) in values.iter().enumerate() {
            for (j, v2) in values.iter().enumerate() {
                if i != j {
                    assert_ne!(v1, v2, "Duplicate code value found");
                }
            }
        }
    }

    #[test]
    fn test_exit_code_follows_unix_convention() {
        // 0 = success is standard Unix convention
        assert_eq!(ExitCode::Success.code(), 0);
        // All error codes should be non-zero
        assert_ne!(ExitCode::GeneralError.code(), 0);
        assert_ne!(ExitCode::InvalidArgs.code(), 0);
        assert_ne!(ExitCode::InputNotFound.code(), 0);
    }

    // ============ Path Handling Tests ============

    #[test]
    fn test_absolute_input_path() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "/absolute/path/to/file.pdf"])
            .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.input.is_absolute());
        }
    }

    #[test]
    fn test_relative_input_path() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "relative/path/file.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.input.is_relative());
        }
    }

    #[test]
    fn test_path_with_spaces() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "/path/with spaces/document.pdf"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.input.to_string_lossy().contains("with spaces"));
        }
    }

    #[test]
    fn test_path_with_unicode() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "/パス/日本語/ドキュメント.pdf"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.input.to_string_lossy().contains("日本語"));
        }
    }

    #[test]
    fn test_directory_as_output() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "/custom/output/directory",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.output.to_string_lossy().contains("directory"));
        }
    }

    // ============ Flag Combinations Tests ============

    #[test]
    fn test_upscale_flag_explicit_true() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--upscale", "true"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.effective_upscale());
        }
    }

    #[test]
    fn test_upscale_flag_explicit_false() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--upscale",
            "false",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.effective_upscale());
        }
    }

    #[test]
    fn test_deskew_flag_explicit_true() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--deskew", "true"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.effective_deskew());
        }
    }

    #[test]
    fn test_deskew_flag_explicit_false() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--deskew", "false"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.effective_deskew());
        }
    }

    #[test]
    fn test_gpu_flag_explicit_true() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--gpu", "true"])
            .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.effective_gpu());
        }
    }

    #[test]
    fn test_gpu_flag_explicit_false() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--gpu", "false"])
            .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.effective_gpu());
        }
    }

    #[test]
    fn test_no_flags_override_explicit() {
        // --upscale true --no-upscale should result in false (no-* takes precedence)
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--upscale",
            "true",
            "--no-upscale",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.effective_upscale());
        }
    }

    // ============ DPI Value Tests ============

    #[test]
    fn test_dpi_common_values() {
        let dpi_values = [72, 150, 300, 600, 1200];
        for dpi in dpi_values {
            let cli = Cli::try_parse_from([
                "superbook-pdf",
                "convert",
                "input.pdf",
                "--dpi",
                &dpi.to_string(),
            ])
            .unwrap();
            if let Commands::Convert(args) = cli.command {
                assert_eq!(args.dpi, dpi);
            }
        }
    }

    #[test]
    fn test_dpi_minimum() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--dpi", "1"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.dpi, 1);
        }
    }

    #[test]
    fn test_dpi_very_high() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--dpi", "2400"])
            .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.dpi, 2400);
        }
    }

    // ============ Progress Bar Additional Tests ============

    #[test]
    fn test_progress_bar_zero_total() {
        let pb = create_progress_bar(0);
        assert_eq!(pb.length(), Some(0));
    }

    #[test]
    fn test_progress_bar_large_total() {
        let pb = create_progress_bar(1_000_000);
        assert_eq!(pb.length(), Some(1_000_000));
        pb.set_position(500_000);
        assert_eq!(pb.position(), 500_000);
    }

    #[test]
    fn test_spinner_empty_message() {
        let spinner = create_spinner("");
        assert_eq!(spinner.message(), "");
    }

    #[test]
    fn test_spinner_unicode_message() {
        let spinner = create_spinner("処理中... 🔄");
        assert!(spinner.message().contains("処理中"));
    }

    #[test]
    fn test_page_progress_bar_single_page() {
        let pb = create_page_progress_bar(1);
        assert_eq!(pb.length(), Some(1));
        pb.set_position(0);
        pb.set_message("page_0.png");
        pb.finish_with_message("Done");
    }

    #[test]
    fn test_page_progress_bar_many_pages() {
        let pb = create_page_progress_bar(1000);
        assert_eq!(pb.length(), Some(1000));
        pb.set_position(999);
        assert_eq!(pb.position(), 999);
    }

    // ============ Short Flag Tests ============

    #[test]
    fn test_short_flag_o_ocr() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-o"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.ocr);
        }
    }

    #[test]
    fn test_short_flag_m_margin() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-m", "2.0"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.margin_trim, 2.0);
        }
    }

    #[test]
    fn test_short_flag_t_threads() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-t", "8"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.thread_count(), 8);
        }
    }

    #[test]
    fn test_short_flag_g_gpu() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-g", "false"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.effective_gpu());
        }
    }

    #[test]
    fn test_short_flag_q_quiet() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-q"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.quiet);
        }
    }

    // ============ Input Path Extension Tests ============

    #[test]
    fn test_various_input_extensions() {
        let extensions = ["pdf", "PDF", "Pdf"];
        for ext in extensions {
            let path = format!("document.{}", ext);
            let cli = Cli::try_parse_from(["superbook-pdf", "convert", &path]).unwrap();
            if let Commands::Convert(args) = cli.command {
                assert!(args.input.to_string_lossy().ends_with(ext));
            }
        }
    }

    #[test]
    fn test_input_without_extension() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "document"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.input, PathBuf::from("document"));
        }
    }

    // ============ Effective Methods Tests ============

    #[test]
    fn test_effective_methods_all_enabled() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--upscale",
            "true",
            "--deskew",
            "true",
            "--gpu",
            "true",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.effective_upscale());
            assert!(args.effective_deskew());
            assert!(args.effective_gpu());
        }
    }

    #[test]
    fn test_effective_methods_all_disabled() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--no-upscale",
            "--no-deskew",
            "--no-gpu",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.effective_upscale());
            assert!(!args.effective_deskew());
            assert!(!args.effective_gpu());
        }
    }

    // ============ Command Metadata Tests ============

    #[test]
    fn test_command_name() {
        let cmd = Cli::command();
        assert_eq!(cmd.get_name(), "superbook-pdf");
    }

    #[test]
    fn test_command_about() {
        let cmd = Cli::command();
        let about = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
        assert!(!about.is_empty());
    }

    #[test]
    fn test_subcommands_present() {
        let cmd = Cli::command();
        let subcommands: Vec<_> = cmd.get_subcommands().collect();
        assert!(subcommands.len() >= 2); // convert and info
    }

    // ============ Error Message Tests ============

    #[test]
    fn test_invalid_dpi_format_error() {
        let result = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--dpi",
            "not_a_number",
        ]);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // Error should mention invalid value
        assert!(
            err_msg.contains("invalid") || err_msg.contains("error") || err_msg.contains("parse")
        );
    }

    #[test]
    fn test_invalid_thread_format_error() {
        let result =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--threads", "abc"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_margin_format_error() {
        let result = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--margin-trim",
            "invalid",
        ]);
        assert!(result.is_err());
    }

    // ============ Concurrency Tests ============

    #[test]
    fn test_cli_types_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Cli>();
        assert_send_sync::<Commands>();
        assert_send_sync::<ConvertArgs>();
        assert_send_sync::<ExitCode>();
    }

    #[test]
    fn test_concurrent_cli_parsing() {
        use std::thread;
        let handles: Vec<_> = (0..4)
            .map(|i| {
                thread::spawn(move || {
                    let cli = Cli::try_parse_from([
                        "superbook-pdf",
                        "convert",
                        &format!("input_{}.pdf", i),
                        "--dpi",
                        &(300 + i * 100).to_string(),
                    ])
                    .unwrap();
                    if let Commands::Convert(args) = cli.command {
                        args.dpi
                    } else {
                        0
                    }
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.len(), 4);
        for (i, dpi) in results.iter().enumerate() {
            assert_eq!(*dpi, 300 + (i as u32) * 100);
        }
    }

    #[test]
    fn test_exit_code_thread_transfer() {
        use std::thread;

        let codes = vec![
            ExitCode::Success,
            ExitCode::GeneralError,
            ExitCode::InvalidArgs,
            ExitCode::InputNotFound,
        ];

        let handles: Vec<_> = codes
            .into_iter()
            .map(|code| {
                thread::spawn(move || {
                    let c = code.code();
                    let d = code.description().to_string();
                    (c, d)
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.len(), 4);
        assert_eq!(results[0].0, 0);
        assert_eq!(results[1].0, 1);
        assert_eq!(results[2].0, 2);
        assert_eq!(results[3].0, 3);
    }

    #[test]
    fn test_convert_args_clone_across_threads() {
        use std::sync::Arc;
        use std::thread;

        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--dpi", "600"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            let shared = Arc::new(args);

            let handles: Vec<_> = (0..4)
                .map(|_| {
                    let args_clone = Arc::clone(&shared);
                    thread::spawn(move || {
                        assert_eq!(args_clone.dpi, 600);
                        args_clone.effective_deskew()
                    })
                })
                .collect();

            for handle in handles {
                let _ = handle.join().unwrap();
            }
        }
    }

    // ============ Boundary Value Tests ============

    #[test]
    fn test_dpi_boundary_minimum() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--dpi", "1"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.dpi, 1);
        }
    }

    #[test]
    fn test_dpi_boundary_maximum() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--dpi", "2400"])
            .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.dpi, 2400);
        }
    }

    #[test]
    fn test_threads_boundary_one() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--threads", "1"])
            .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.threads, Some(1));
        }
    }

    #[test]
    fn test_threads_boundary_large() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--threads", "128"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.threads, Some(128));
        }
    }

    #[test]
    fn test_margin_trim_boundary_zero() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--margin-trim",
            "0.0",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.margin_trim, 0.0);
        }
    }

    #[test]
    fn test_margin_trim_boundary_large() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--margin-trim",
            "50.0",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.margin_trim, 50.0);
        }
    }

    #[test]
    fn test_verbose_boundary_maximum() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "-vvvvvvvv", // 8 v's
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.verbose, 8);
        }
    }

    #[test]
    fn test_exit_code_all_variants() {
        let codes = [
            ExitCode::Success,
            ExitCode::GeneralError,
            ExitCode::InvalidArgs,
            ExitCode::InputNotFound,
            ExitCode::OutputError,
            ExitCode::ProcessingError,
            ExitCode::GpuError,
            ExitCode::ExternalToolError,
        ];

        for code in codes {
            assert!(!code.description().is_empty());
            assert!(code.code() <= 7);
        }
    }

    #[test]
    fn test_path_with_special_characters() {
        let paths = [
            "file with spaces.pdf",
            "日本語ファイル.pdf",
            "file-with-dashes.pdf",
            "file_with_underscores.pdf",
        ];

        for path in paths {
            let cli = Cli::try_parse_from(["superbook-pdf", "convert", path]).unwrap();
            if let Commands::Convert(args) = cli.command {
                assert_eq!(args.input.to_string_lossy(), path);
            }
        }
    }

    #[test]
    fn test_output_path_variants() {
        let outputs = ["./output", "/tmp/out", "../parent/out", "relative/path"];

        for output in outputs {
            let cli =
                Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", output]).unwrap();
            if let Commands::Convert(args) = cli.command {
                assert_eq!(args.output.to_string_lossy(), output);
            }
        }
    }
}
