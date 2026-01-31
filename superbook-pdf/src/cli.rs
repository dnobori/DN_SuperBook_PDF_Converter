//! CLI interface module
//!
//! Provides command-line interface using clap derive macros.

use clap::{Args, Parser, Subcommand};
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
#[command(about = "High-quality PDF converter for scanned books")]
#[command(after_help = r#"
Quick Start:
  superbook-pdf convert input.pdf -o output/           # 基本変換
  superbook-pdf convert input.pdf -o output/ --advanced --ocr  # 高品質変換
  superbook-pdf serve --port 8080                      # Web UI起動

詳細は `superbook-pdf <COMMAND> --help` を参照
"#)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Convert PDF files with AI enhancement
    Convert(ConvertArgs),
    /// Reprocess failed pages from a previous conversion
    Reprocess(ReprocessArgs),
    /// Convert PDF to Markdown format
    Markdown(MarkdownArgs),
    /// Show system information
    Info,
    /// Show cache information for a processed file
    CacheInfo(CacheInfoArgs),
    /// Start web server for browser-based conversion
    #[cfg(feature = "web")]
    Serve(ServeArgs),
}

/// Arguments for the cache-info command
#[derive(Args, Debug)]
pub struct CacheInfoArgs {
    /// Path to the output PDF file (to show cache info)
    #[arg(value_name = "OUTPUT_PDF")]
    pub output_pdf: std::path::PathBuf,
}

/// Shadow removal mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ShadowRemovalMode {
    /// No shadow removal
    None,
    /// Automatic detection and removal
    #[default]
    Auto,
    /// Remove shadow from left edge only
    Left,
    /// Remove shadow from right edge only
    Right,
    /// Remove shadow from both edges
    Both,
}

/// Deblur algorithm for CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum DeblurAlgorithmCli {
    /// Unsharp mask (fast, local processing)
    #[default]
    UnsharpMask,
    /// NAFNet AI deblurring (requires GPU)
    Nafnet,
    /// DeblurGAN-v2 AI deblurring (requires GPU)
    DeblurganV2,
}

/// Text direction option for CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum TextDirectionCli {
    /// Automatic detection
    #[default]
    Auto,
    /// Horizontal (left-to-right, top-to-bottom)
    Horizontal,
    /// Vertical (right-to-left, top-to-bottom) for Japanese
    Vertical,
}

/// Validation provider for CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ValidationProviderCli {
    /// Claude API
    Claude,
    /// OpenAI API
    Openai,
    /// Local LLM endpoint
    #[default]
    Local,
}

/// Arguments for the markdown command (Issue #36)
#[derive(Args, Debug)]
#[command(after_help = r#"
Examples:
  # 基本的なMarkdown変換
  superbook-pdf markdown input.pdf -o output/

  # 外部API検証付き
  superbook-pdf markdown input.pdf -o output/ --validate --api-provider claude

  # 縦書き指定
  superbook-pdf markdown input.pdf -o output/ --text-direction vertical

  # 画像抽出なし
  superbook-pdf markdown input.pdf -o output/ --no-extract-images
"#)]
pub struct MarkdownArgs {
    /// Input PDF file
    pub input: PathBuf,

    /// Output directory
    #[arg(short = 'o', long = "output", default_value = "./markdown_output")]
    pub output: PathBuf,

    /// Text direction for reading order
    #[arg(long, value_enum, default_value = "auto")]
    pub text_direction: TextDirectionCli,

    /// Extract images from PDF
    #[arg(long, default_value_t = true)]
    #[arg(action = clap::ArgAction::Set)]
    pub extract_images: bool,

    /// Disable image extraction
    #[arg(long = "no-extract-images")]
    #[arg(action = clap::ArgAction::SetTrue)]
    no_extract_images: bool,

    /// Enable table detection
    #[arg(long, default_value_t = true)]
    #[arg(action = clap::ArgAction::Set)]
    pub detect_tables: bool,

    /// Disable table detection
    #[arg(long = "no-detect-tables")]
    #[arg(action = clap::ArgAction::SetTrue)]
    no_detect_tables: bool,

    /// Include page numbers in output
    #[arg(long)]
    pub include_page_numbers: bool,

    /// Generate metadata JSON file
    #[arg(long)]
    pub generate_metadata: bool,

    /// Enable validation of output Markdown
    #[arg(long)]
    pub validate: bool,

    /// API provider for validation
    #[arg(long, value_enum, default_value = "local")]
    pub api_provider: ValidationProviderCli,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress progress output
    #[arg(short, long)]
    pub quiet: bool,
}

impl MarkdownArgs {
    /// Get effective extract_images setting
    pub fn effective_extract_images(&self) -> bool {
        self.extract_images && !self.no_extract_images
    }

    /// Get effective detect_tables setting
    pub fn effective_detect_tables(&self) -> bool {
        self.detect_tables && !self.no_detect_tables
    }
}

/// Arguments for the reprocess command
#[derive(Args, Debug, Clone)]
pub struct ReprocessArgs {
    /// PDF file or state file path (.superbook-state.json)
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    /// Specific pages to retry (comma-separated, 0-indexed)
    #[arg(short, long, value_delimiter = ',')]
    pub pages: Option<Vec<usize>>,

    /// Maximum retry attempts per page
    #[arg(long, default_value = "3")]
    pub max_retries: u32,

    /// Force reprocess all failed pages
    #[arg(short, long)]
    pub force: bool,

    /// Show status only, don't process
    #[arg(long)]
    pub status: bool,

    /// Output directory (only if input is PDF, not state file)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Keep intermediate files
    #[arg(long)]
    pub keep_intermediates: bool,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress progress output
    #[arg(short, long)]
    pub quiet: bool,
}

impl ReprocessArgs {
    /// Check if the input is a state file
    pub fn is_state_file(&self) -> bool {
        self.input
            .to_string_lossy()
            .ends_with(".superbook-state.json")
    }

    /// Get page indices to reprocess (empty means all failed)
    pub fn page_indices(&self) -> Vec<usize> {
        self.pages.clone().unwrap_or_default()
    }
}

/// Arguments for the serve command (web server)
#[cfg(feature = "web")]
#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(short, long, default_value = "8080")]
    pub port: u16,

    /// Address to bind to
    #[arg(short, long, default_value = "127.0.0.1")]
    pub bind: String,

    /// Maximum upload size in MB
    #[arg(long, default_value = "500")]
    pub upload_limit: usize,

    /// Job timeout in seconds
    #[arg(long, default_value = "3600")]
    pub job_timeout: u64,

    /// Enable CORS (default: enabled)
    #[arg(long, default_value = "true")]
    pub cors: bool,

    /// Allowed CORS origins (can be specified multiple times)
    #[arg(long = "cors-origin")]
    pub cors_origins: Vec<String>,

    /// Disable CORS
    #[arg(long)]
    pub no_cors: bool,
}

/// Arguments for the convert command
#[derive(clap::Args, Debug)]
#[command(after_help = r#"
Examples:
  # 基本的な変換
  superbook-pdf convert input.pdf -o output/

  # AI超解像 + OCR付き高品質変換
  superbook-pdf convert input.pdf -o output/ --advanced --ocr

  # GPU無効化 (CPUのみ)
  superbook-pdf convert input.pdf -o output/ --no-gpu

  # 最初の10ページのみテスト
  superbook-pdf convert input.pdf -o output/ --max-pages 10

  # 詳細ログ出力
  superbook-pdf convert input.pdf -o output/ -vvv
"#)]
pub struct ConvertArgs {
    /// Input PDF file or directory
    pub input: PathBuf,

    /// Output directory
    #[arg(short = 'o', long = "output", default_value = "./output")]
    pub output: PathBuf,

    /// Configuration file path (TOML format)
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,

    /// Enable Japanese OCR (YomiToku)
    #[arg(long)]
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

    /// Output DPI (1-4800)
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u32).range(1..=4800))]
    pub dpi: u32,

    /// JPEG quality for PDF image compression (1-100, higher = better quality, larger file)
    #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub jpeg_quality: u8,

    /// Number of parallel threads
    #[arg(short = 't', long)]
    pub threads: Option<usize>,

    /// Chunk size for memory-controlled parallel processing (0 = process all at once)
    #[arg(long, default_value_t = 0)]
    pub chunk_size: usize,

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

    // === Phase 6: Advanced processing options ===
    /// Enable internal resolution normalization (4960x7016)
    #[arg(long)]
    pub internal_resolution: bool,

    /// Enable global color correction
    #[arg(long)]
    pub color_correction: bool,

    /// Enable page number offset alignment
    #[arg(long)]
    pub offset_alignment: bool,

    /// Output height in pixels (default: 3508)
    #[arg(long, default_value_t = 3508)]
    pub output_height: u32,

    /// Enable advanced processing for best quality output
    /// (includes: internal resolution normalization, color correction, offset alignment)
    #[arg(long)]
    pub advanced: bool,

    /// Skip files if output already exists
    #[arg(long)]
    pub skip_existing: bool,

    /// Force re-processing even if cache is valid
    #[arg(long, short = 'f')]
    pub force: bool,

    // === Content-Aware Margin Options (Issue #32) ===
    /// Enable content-aware margin detection to prevent text clipping
    #[arg(long, default_value_t = true)]
    #[arg(action = clap::ArgAction::Set)]
    pub content_aware_margins: bool,

    /// Disable content-aware margin detection
    #[arg(long = "no-content-aware-margins")]
    #[arg(action = clap::ArgAction::SetTrue)]
    no_content_aware_margins: bool,

    /// Safety buffer percentage for content-aware margins (0.0-5.0)
    #[arg(long, default_value_t = 0.5)]
    pub margin_safety: f32,

    /// Use aggressive trimming (may risk text clipping)
    #[arg(long)]
    pub aggressive_trim: bool,

    // === Shadow Removal Options (Issue #33) ===
    /// Shadow removal mode for book binding areas
    #[arg(long, value_enum, default_value = "auto")]
    pub shadow_removal: ShadowRemovalMode,

    // === Marker Removal Options (Issue #34) ===
    /// Enable highlighter/marker removal
    #[arg(long)]
    pub remove_markers: bool,

    /// Colors to remove (comma-separated: yellow,pink,green,blue,orange)
    #[arg(long, value_delimiter = ',', default_value = "yellow,pink,green,blue")]
    pub marker_colors: Vec<String>,

    // === Deblur Options (Issue #35) ===
    /// Enable blur detection and correction
    #[arg(long)]
    pub deblur: bool,

    /// Deblur algorithm to use
    #[arg(long, value_enum, default_value = "unsharp-mask")]
    pub deblur_algorithm: DeblurAlgorithmCli,

    // === Debug options ===
    /// Maximum pages to process (for debugging)
    #[arg(long)]
    pub max_pages: Option<usize>,

    /// Save intermediate debug images
    #[arg(long)]
    pub save_debug: bool,
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

    /// Get effective internal resolution setting (considering --advanced flag)
    pub fn effective_internal_resolution(&self) -> bool {
        self.internal_resolution || self.advanced
    }

    /// Get effective color correction setting (considering --advanced flag)
    pub fn effective_color_correction(&self) -> bool {
        self.color_correction || self.advanced
    }

    /// Get effective offset alignment setting (considering --advanced flag)
    pub fn effective_offset_alignment(&self) -> bool {
        self.offset_alignment || self.advanced
    }

    /// Get effective content-aware margins setting
    pub fn effective_content_aware_margins(&self) -> bool {
        self.content_aware_margins && !self.no_content_aware_margins
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

    // TC-CLI-001: ヘルプ表示
    #[test]
    fn test_help_display() {
        let mut cmd = Cli::command();
        let help = cmd.render_help().to_string();
        assert!(help.contains("superbook-pdf"));
        assert!(help.contains("convert"));
    }

    // TC-CLI-002: バージョン表示
    #[test]
    fn test_version_display() {
        let cmd = Cli::command();
        let version = cmd.get_version().unwrap_or("unknown");
        assert!(!version.is_empty());
    }

    // TC-CLI-003: 入力ファイルなしエラー
    #[test]
    fn test_missing_input_error() {
        let result = Cli::try_parse_from(["superbook-pdf", "convert"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("required"));
    }

    // TC-CLI-004: 存在しないファイルエラー（パース時はエラーにならない、実行時にチェック）
    #[test]
    fn test_nonexistent_file_parse() {
        // Note: CLI parsing accepts any path, existence check is at runtime
        let result = Cli::try_parse_from(["superbook-pdf", "convert", "/nonexistent/file.pdf"]);
        assert!(result.is_ok()); // Parsing succeeds
    }

    // TC-CLI-005: オプション解析
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

    // TC-CLI-006: デフォルト値
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
        let cloned = code;
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

    // Test output path argument (--output / -o)
    #[test]
    fn test_output_path() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-o", "/custom/output"])
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
        let cloned = original;
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
            "-o",
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
    fn test_short_flag_o_output() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-o", "/custom/out"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.output, PathBuf::from("/custom/out"));
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

    // ============ JPEG Quality Tests ============

    #[test]
    fn test_jpeg_quality_default() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.jpeg_quality, 90);
        }
    }

    #[test]
    fn test_jpeg_quality_custom() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--jpeg-quality",
            "75",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.jpeg_quality, 75);
        }
    }

    #[test]
    fn test_jpeg_quality_boundary_minimum() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--jpeg-quality",
            "1",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.jpeg_quality, 1);
        }
    }

    #[test]
    fn test_jpeg_quality_boundary_maximum() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--jpeg-quality",
            "100",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.jpeg_quality, 100);
        }
    }

    #[test]
    fn test_jpeg_quality_invalid_zero() {
        let result = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--jpeg-quality",
            "0",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_jpeg_quality_invalid_over_100() {
        let result = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--jpeg-quality",
            "101",
        ]);
        assert!(result.is_err());
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

    // ============ Chunk size tests ============

    #[test]
    fn test_chunk_size_default() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.chunk_size, 0);
        }
    }

    #[test]
    fn test_chunk_size_explicit() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--chunk-size", "10"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.chunk_size, 10);
        }
    }

    #[test]
    fn test_chunk_size_large() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--chunk-size", "100"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.chunk_size, 100);
        }
    }

    #[test]
    fn test_chunk_size_with_threads() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--threads",
            "4",
            "--chunk-size",
            "20",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.threads, Some(4));
            assert_eq!(args.chunk_size, 20);
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
                Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-o", output]).unwrap();
            if let Commands::Convert(args) = cli.command {
                assert_eq!(args.output.to_string_lossy(), output);
            }
        }
    }

    // ============ Phase 6: Advanced Processing Options Tests ============

    #[test]
    fn test_internal_resolution_flag() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--internal-resolution",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.internal_resolution);
            assert!(args.effective_internal_resolution());
        }
    }

    #[test]
    fn test_color_correction_flag() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--color-correction",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.color_correction);
            assert!(args.effective_color_correction());
        }
    }

    #[test]
    fn test_offset_alignment_flag() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--offset-alignment",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.offset_alignment);
            assert!(args.effective_offset_alignment());
        }
    }

    #[test]
    fn test_output_height_default() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.output_height, 3508);
        }
    }

    #[test]
    fn test_output_height_custom() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--output-height",
            "7016",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.output_height, 7016);
        }
    }

    #[test]
    fn test_advanced_flag_enables_all() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--advanced"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.advanced);
            assert!(args.effective_internal_resolution());
            assert!(args.effective_color_correction());
            assert!(args.effective_offset_alignment());
        }
    }

    #[test]
    fn test_advanced_flag_combined() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--advanced",
            "--output-height",
            "4000",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.advanced);
            assert_eq!(args.output_height, 4000);
            assert!(args.effective_internal_resolution());
        }
    }

    #[test]
    fn test_individual_flags_without_advanced() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.advanced);
            assert!(!args.internal_resolution);
            assert!(!args.color_correction);
            assert!(!args.offset_alignment);
            assert!(!args.effective_internal_resolution());
            assert!(!args.effective_color_correction());
            assert!(!args.effective_offset_alignment());
        }
    }

    // ============ Skip Existing Option Tests ============

    #[test]
    fn test_skip_existing_default() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.skip_existing);
        }
    }

    #[test]
    fn test_skip_existing_enabled() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--skip-existing"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.skip_existing);
        }
    }

    #[test]
    fn test_skip_existing_with_other_options() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--skip-existing",
            "--advanced",
            "-v",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.skip_existing);
            assert!(args.advanced);
            assert_eq!(args.verbose, 1);
        }
    }

    // ============ Debug Options Tests ============

    #[test]
    fn test_max_pages_default() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.max_pages.is_none());
        }
    }

    #[test]
    fn test_max_pages_explicit() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--max-pages", "10"])
                .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.max_pages, Some(10));
        }
    }

    #[test]
    fn test_save_debug_default() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.save_debug);
        }
    }

    #[test]
    fn test_save_debug_enabled() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--save-debug"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.save_debug);
        }
    }

    #[test]
    fn test_debug_options_combined() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--max-pages",
            "5",
            "--save-debug",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert_eq!(args.max_pages, Some(5));
            assert!(args.save_debug);
        }
    }

    // ============ Force Option Tests ============

    #[test]
    fn test_force_default() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(!args.force);
        }
    }

    #[test]
    fn test_force_long() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "--force"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.force);
        }
    }

    #[test]
    fn test_force_short() {
        let cli = Cli::try_parse_from(["superbook-pdf", "convert", "input.pdf", "-f"]).unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.force);
        }
    }

    #[test]
    fn test_force_with_skip_existing() {
        // Both flags can be used together (--force takes precedence)
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "convert",
            "input.pdf",
            "--skip-existing",
            "--force",
        ])
        .unwrap();
        if let Commands::Convert(args) = cli.command {
            assert!(args.skip_existing);
            assert!(args.force);
        }
    }

    // ============ Cache Info Command Tests ============

    #[test]
    fn test_cache_info_command() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "cache-info", "output.pdf"]).unwrap();
        if let Commands::CacheInfo(args) = cli.command {
            assert_eq!(args.output_pdf, PathBuf::from("output.pdf"));
        } else {
            panic!("Expected CacheInfo command");
        }
    }

    #[test]
    fn test_cache_info_with_path() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "cache-info",
            "/path/to/output.pdf",
        ])
        .unwrap();
        if let Commands::CacheInfo(args) = cli.command {
            assert_eq!(args.output_pdf, PathBuf::from("/path/to/output.pdf"));
        } else {
            panic!("Expected CacheInfo command");
        }
    }

    #[test]
    fn test_cache_info_missing_path() {
        let result = Cli::try_parse_from(["superbook-pdf", "cache-info"]);
        assert!(result.is_err());
    }

    // ============ Reprocess Command Tests ============

    #[test]
    fn test_reprocess_command_basic() {
        let cli = Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf"]).unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert_eq!(args.input, PathBuf::from("input.pdf"));
            assert!(args.pages.is_none());
            assert_eq!(args.max_retries, 3);
            assert!(!args.force);
            assert!(!args.status);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_status() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf", "--status"]).unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert!(args.status);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_pages() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "reprocess",
            "input.pdf",
            "--pages",
            "1,3,5,10",
        ])
        .unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert_eq!(args.pages, Some(vec![1, 3, 5, 10]));
            assert_eq!(args.page_indices(), vec![1, 3, 5, 10]);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_max_retries() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "reprocess",
            "input.pdf",
            "--max-retries",
            "5",
        ])
        .unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert_eq!(args.max_retries, 5);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_force() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf", "--force"]).unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert!(args.force);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_force_short() {
        let cli = Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf", "-f"]).unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert!(args.force);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_output() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "reprocess",
            "input.pdf",
            "--output",
            "/custom/output",
        ])
        .unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert_eq!(args.output, Some(PathBuf::from("/custom/output")));
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_keep_intermediates() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "reprocess",
            "input.pdf",
            "--keep-intermediates",
        ])
        .unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert!(args.keep_intermediates);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_verbose() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf", "-vvv"]).unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert_eq!(args.verbose, 3);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_with_quiet() {
        let cli =
            Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf", "-q"]).unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert!(args.quiet);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_state_file_detection() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "reprocess",
            "output/.superbook-state.json",
        ])
        .unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert!(args.is_state_file());
        } else {
            panic!("Expected Reprocess command");
        }

        let cli2 = Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf"]).unwrap();
        if let Commands::Reprocess(args) = cli2.command {
            assert!(!args.is_state_file());
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_page_indices_empty() {
        let cli = Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf"]).unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert!(args.page_indices().is_empty());
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_full_options() {
        let cli = Cli::try_parse_from([
            "superbook-pdf",
            "reprocess",
            "input.pdf",
            "--pages",
            "1,2,3",
            "--max-retries",
            "5",
            "--force",
            "--output",
            "/custom/output",
            "--keep-intermediates",
            "-vv",
        ])
        .unwrap();
        if let Commands::Reprocess(args) = cli.command {
            assert_eq!(args.input, PathBuf::from("input.pdf"));
            assert_eq!(args.pages, Some(vec![1, 2, 3]));
            assert_eq!(args.max_retries, 5);
            assert!(args.force);
            assert_eq!(args.output, Some(PathBuf::from("/custom/output")));
            assert!(args.keep_intermediates);
            assert_eq!(args.verbose, 2);
        } else {
            panic!("Expected Reprocess command");
        }
    }

    #[test]
    fn test_reprocess_missing_input() {
        let result = Cli::try_parse_from(["superbook-pdf", "reprocess"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_reprocess_debug_impl() {
        let cli = Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf"]).unwrap();
        let debug_str = format!("{:?}", cli.command);
        assert!(debug_str.contains("Reprocess"));
    }

    #[test]
    fn test_reprocess_args_clone() {
        let cli = Cli::try_parse_from(["superbook-pdf", "reprocess", "input.pdf", "--force"])
            .unwrap();
        if let Commands::Reprocess(args) = cli.command {
            let cloned = args.clone();
            assert_eq!(args.input, cloned.input);
            assert_eq!(args.force, cloned.force);
        } else {
            panic!("Expected Reprocess command");
        }
    }
}
