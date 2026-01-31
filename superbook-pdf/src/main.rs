//! superbook-pdf - High-quality PDF converter for scanned books
//!
//! CLI entry point

use clap::Parser;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;
use superbook_pdf::{
    exit_codes,
    // Cache module
    CacheDigest, ProcessingCache, should_skip_processing,
    // CLI
    CacheInfoArgs, Cli, Commands, ConvertArgs, MarkdownArgs, ReprocessArgs,
    // Config
    CliOverrides, Config,
    // Pipeline
    PdfPipeline, ProgressCallback,
    // Progress tracking
    ProgressTracker,
    // Reprocess
    PageStatus, ReprocessOptions, ReprocessState,
};

#[cfg(feature = "web")]
use superbook_pdf::{ServeArgs, ServerConfig, WebServer};

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Convert(args) => run_convert(&args),
        Commands::Reprocess(args) => run_reprocess(&args),
        Commands::Markdown(args) => run_markdown(&args),
        Commands::Info => run_info(),
        Commands::CacheInfo(args) => run_cache_info(&args),
        #[cfg(feature = "web")]
        Commands::Serve(args) => run_serve(&args),
    };

    std::process::exit(match result {
        Ok(()) => exit_codes::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            exit_codes::GENERAL_ERROR
        }
    });
}

// ============ Progress Callback Implementation ============

/// Verbose progress callback for CLI output
struct VerboseProgress {
    verbose_level: u32,
}

impl VerboseProgress {
    fn new(verbose_level: u32) -> Self {
        Self { verbose_level }
    }

    /// Check if step messages should be shown (level >= 1)
    #[allow(dead_code)]
    fn should_show_steps(&self) -> bool {
        self.verbose_level > 0
    }

    /// Check if progress messages should be shown (level >= 1)
    #[allow(dead_code)]
    fn should_show_progress(&self) -> bool {
        self.verbose_level > 0
    }

    /// Check if debug messages should be shown (level >= 3, i.e., -vvv)
    #[allow(dead_code)]
    fn should_show_debug(&self) -> bool {
        self.verbose_level > 2
    }
}

impl ProgressCallback for VerboseProgress {
    fn on_step_start(&self, step: &str) {
        if self.verbose_level > 0 {
            println!("  {}", step);
        }
    }

    fn on_step_progress(&self, current: usize, total: usize) {
        if self.verbose_level > 0 {
            print!("\r    Progress: {}/{}", current, total);
            std::io::stdout().flush().ok();
        }
    }

    fn on_step_complete(&self, step: &str, message: &str) {
        if self.verbose_level > 0 {
            println!("    {}: {}", step, message);
        }
    }

    fn on_debug(&self, message: &str) {
        if self.should_show_debug() {
            println!("    [DEBUG] {}", message);
        }
    }
}

// ============ Convert Command ============

fn run_convert(args: &ConvertArgs) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Instant::now();

    // Validate input path
    if !args.input.exists() {
        eprintln!("Error: Input path does not exist: {}", args.input.display());
        std::process::exit(exit_codes::INPUT_NOT_FOUND);
    }

    // Collect PDF files to process
    let pdf_files = collect_pdf_files(&args.input)?;
    if pdf_files.is_empty() {
        eprintln!("Error: No PDF files found in input path");
        std::process::exit(exit_codes::INPUT_NOT_FOUND);
    }

    // Load config file if specified, otherwise use default
    let file_config = match &args.config {
        Some(config_path) => {
            match Config::load_from_path(config_path) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("Warning: Failed to load config file: {}", e);
                    Config::default()
                }
            }
        }
        None => Config::load().unwrap_or_default(),
    };

    // Create CLI overrides from command-line arguments
    let cli_overrides = create_cli_overrides(args);

    // Merge config file with CLI arguments (CLI takes precedence)
    let pipeline_config = file_config.merge_with_cli(&cli_overrides);
    let pipeline = PdfPipeline::new(pipeline_config);

    if args.dry_run {
        print_execution_plan(args, &pdf_files, pipeline.config());
        return Ok(());
    }

    // Create output directory
    std::fs::create_dir_all(&args.output)?;

    let verbose = args.verbose > 0;

    // Create progress callback
    let progress = VerboseProgress::new(args.verbose.into());

    // Pre-compute options JSON for caching
    let options_json = pipeline.config().to_json();

    // Track processing results
    let mut ok_count = 0usize;
    let mut skip_count = 0usize;
    let mut error_count = 0usize;

    // Process each PDF file
    for (idx, pdf_path) in pdf_files.iter().enumerate() {
        let output_pdf = pipeline.get_output_path(pdf_path, &args.output);

        // Check cache for smart skipping
        if args.skip_existing && !args.force {
            if output_pdf.exists() {
                if verbose {
                    println!(
                        "[{}/{}] Skipping (exists): {}",
                        idx + 1,
                        pdf_files.len(),
                        pdf_path.display()
                    );
                }
                skip_count += 1;
                continue;
            }
        } else if !args.force {
            if let Some(cache) = should_skip_processing(pdf_path, &output_pdf, &options_json, false) {
                if verbose {
                    println!(
                        "[{}/{}] Skipping (cached, {} pages): {}",
                        idx + 1,
                        pdf_files.len(),
                        cache.result.page_count,
                        pdf_path.display()
                    );
                }
                skip_count += 1;
                continue;
            }
        }

        if verbose {
            println!(
                "[{}/{}] Processing: {}",
                idx + 1,
                pdf_files.len(),
                pdf_path.display()
            );
        }

        // Process using pipeline
        match pipeline.process_with_progress(pdf_path, &args.output, &progress) {
            Ok(result) => {
                ok_count += 1;

                // Save cache after successful processing
                if let Ok(digest) = CacheDigest::new(pdf_path, &options_json) {
                    let cache_result = result.to_cache_result();
                    let cache = ProcessingCache::new(digest, cache_result);
                    let _ = cache.save(&output_pdf);
                }

                if verbose {
                    println!(
                        "    Completed: {} pages, {:.2}s, {} bytes",
                        result.page_count,
                        result.elapsed_seconds,
                        result.output_size
                    );
                }
            }
            Err(e) => {
                eprintln!("Error processing {}: {}", pdf_path.display(), e);
                error_count += 1;
            }
        }
    }

    let elapsed = start_time.elapsed();

    // Print summary
    if !args.quiet {
        ProgressTracker::print_summary(pdf_files.len(), ok_count, skip_count, error_count);
        println!("Total time: {:.2}s", elapsed.as_secs_f64());
    }

    if error_count > 0 {
        return Err(format!("{} file(s) failed to process", error_count).into());
    }

    Ok(())
}

// ============ Helper Functions ============

/// Create CLI overrides from ConvertArgs
///
/// Only override config file values when CLI explicitly sets a non-default value.
/// This allows config files to provide defaults that aren't overridden by clap defaults.
fn create_cli_overrides(args: &ConvertArgs) -> CliOverrides {
    let mut overrides = CliOverrides::new();

    // CLI defaults - only override if user explicitly changed these
    const DEFAULT_DPI: u32 = 300;
    const DEFAULT_MARGIN_TRIM: f32 = 0.5;
    const DEFAULT_OUTPUT_HEIGHT: u32 = 3508;
    const DEFAULT_JPEG_QUALITY: u8 = 90;

    // Basic options - only set if they differ from defaults
    if args.dpi != DEFAULT_DPI {
        overrides.dpi = Some(args.dpi);
    }

    // Deskew: override if --no-deskew was used
    if !args.effective_deskew() {
        overrides.deskew = Some(false);
    }

    // Margin trim: override if changed from default
    if (args.margin_trim - DEFAULT_MARGIN_TRIM).abs() > f32::EPSILON {
        overrides.margin_trim = Some(args.margin_trim as f64);
    }

    // Upscale: override if --no-upscale was used
    if !args.effective_upscale() {
        overrides.upscale = Some(false);
    }

    // GPU: override if --no-gpu was used
    if !args.effective_gpu() {
        overrides.gpu = Some(false);
    }

    // OCR: override if explicitly enabled
    if args.ocr {
        overrides.ocr = Some(true);
    }

    // Threads: only set if explicitly provided
    overrides.threads = args.threads;

    // Advanced options - only set if explicitly enabled
    if args.internal_resolution || args.advanced {
        overrides.internal_resolution = Some(true);
    }
    if args.color_correction || args.advanced {
        overrides.color_correction = Some(true);
    }
    if args.offset_alignment || args.advanced {
        overrides.offset_alignment = Some(true);
    }

    // Output height: only set if changed from default
    if args.output_height != DEFAULT_OUTPUT_HEIGHT {
        overrides.output_height = Some(args.output_height);
    }

    // JPEG quality: only set if changed from default
    if args.jpeg_quality != DEFAULT_JPEG_QUALITY {
        overrides.jpeg_quality = Some(args.jpeg_quality);
    }

    // Debug options
    overrides.max_pages = args.max_pages;
    if args.save_debug {
        overrides.save_debug = Some(true);
    }

    overrides
}

/// Collect PDF files from input path (file or directory)
fn collect_pdf_files(input: &PathBuf) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut pdf_files = Vec::new();

    if input.is_file() {
        if input.extension().is_some_and(|ext| ext == "pdf") {
            pdf_files.push(input.clone());
        }
    } else if input.is_dir() {
        for entry in std::fs::read_dir(input)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "pdf") {
                pdf_files.push(path);
            }
        }
        pdf_files.sort();
    }

    Ok(pdf_files)
}

/// Print execution plan for dry-run mode
fn print_execution_plan(args: &ConvertArgs, pdf_files: &[PathBuf], config: &superbook_pdf::PipelineConfig) {
    println!("=== Dry Run - Execution Plan ===");
    println!();
    println!("Input: {}", args.input.display());
    println!("Output: {}", args.output.display());
    println!("Files to process: {}", pdf_files.len());
    println!();
    println!("Pipeline Configuration:");
    println!("  1. Image Extraction (DPI: {})", config.dpi);
    if config.deskew {
        println!("  2. Deskew Correction: ENABLED");
    } else {
        println!("  2. Deskew Correction: DISABLED");
    }
    println!("  3. Margin Trim: {}%", config.margin_trim);
    if config.upscale {
        println!("  4. AI Upscaling (RealESRGAN 2x): ENABLED");
    } else {
        println!("  4. AI Upscaling: DISABLED");
    }
    if config.ocr {
        println!("  5. OCR (YomiToku): ENABLED");
    } else {
        println!("  5. OCR: DISABLED");
    }
    if config.internal_resolution {
        println!("  6. Internal Resolution Normalization (4960x7016): ENABLED");
    }
    if config.color_correction {
        println!("  7. Global Color Correction: ENABLED");
    }
    if config.offset_alignment {
        println!("  8. Page Number Offset Alignment: ENABLED");
    }
    println!("  9. PDF Generation (output height: {})", config.output_height);
    println!();
    println!("Processing Options:");
    println!("  Threads: {}", config.threads.unwrap_or_else(num_cpus::get));
    if args.chunk_size > 0 {
        println!("  Chunk size: {} pages", args.chunk_size);
    } else {
        println!("  Chunk size: unlimited (all pages at once)");
    }
    println!("  GPU: {}", if config.gpu { "YES" } else { "NO" });
    println!("  Skip existing: {}", if args.skip_existing { "YES" } else { "NO" });
    println!("  Force re-process: {}", if args.force { "YES" } else { "NO" });
    println!("  Verbose: {}", args.verbose);
    println!();
    println!("Debug Options:");
    if let Some(max) = config.max_pages {
        println!("  Max pages: {}", max);
    } else {
        println!("  Max pages: unlimited");
    }
    println!("  Save debug images: {}", if config.save_debug { "YES" } else { "NO" });
    println!();
    println!("Files:");
    for (i, file) in pdf_files.iter().enumerate() {
        println!("  {}. {}", i + 1, file.display());
    }
}

// ============ Info Command ============

fn run_info() -> Result<(), Box<dyn std::error::Error>> {
    println!("superbook-pdf v{}", env!("CARGO_PKG_VERSION"));
    println!();

    // System Information
    println!("System Information:");
    println!("  Platform: {}", std::env::consts::OS);
    println!("  Arch: {}", std::env::consts::ARCH);
    println!("  CPUs: {}", num_cpus::get());

    // Memory info (Linux)
    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        if let Some(line) = meminfo.lines().find(|l| l.starts_with("MemTotal:")) {
            if let Some(kb) = line.split_whitespace().nth(1) {
                if let Ok(kb_val) = kb.parse::<u64>() {
                    println!("  Memory: {:.1} GB", kb_val as f64 / 1_048_576.0);
                }
            }
        }
    }

    // External Tools
    println!();
    println!("PDF Extraction Tools:");
    check_tool_with_version("pdftoppm", "Poppler", &["-v"]);
    check_tool_with_version("magick", "ImageMagick", &["--version"]);
    check_tool("gs", "Ghostscript");

    println!();
    println!("OCR Tools:");
    check_tool_with_version("tesseract", "Tesseract", &["--version"]);

    // Python & AI Tools
    println!();
    println!("AI Tools (Python):");
    check_python();

    // GPU Status
    println!();
    println!("GPU Status:");
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name,memory.total,driver_version")
        .arg("--format=csv,noheader")
        .output()
    {
        if output.status.success() {
            let gpu_info = String::from_utf8_lossy(&output.stdout);
            for line in gpu_info.trim().lines() {
                println!("  NVIDIA: {}", line.trim());
            }
        } else {
            println!("  NVIDIA GPU: Not detected");
        }
    } else {
        println!("  NVIDIA GPU: nvidia-smi not found");
    }

    // Config File Locations
    println!();
    println!("Config File Locations:");
    println!("  Local: ./superbook.toml");
    if let Some(config_dir) = dirs::config_dir() {
        println!("  User:  {}", config_dir.join("superbook-pdf/config.toml").display());
    }

    Ok(())
}

fn check_tool(cmd: &str, name: &str) {
    match which::which(cmd) {
        Ok(path) => println!("  {}: {} (found)", name, path.display()),
        Err(_) => println!("  {}: Not found", name),
    }
}

fn check_tool_with_version(cmd: &str, name: &str, version_args: &[&str]) {
    match which::which(cmd) {
        Ok(path) => {
            // Try to get version
            if let Ok(output) = std::process::Command::new(&path).args(version_args).output() {
                let version_str = String::from_utf8_lossy(&output.stdout);
                let first_line = version_str.lines().next().unwrap_or("");
                if !first_line.is_empty() && first_line.len() < 80 {
                    println!("  {}: {} ({})", name, first_line.trim(), path.display());
                } else {
                    println!("  {}: {} (found)", name, path.display());
                }
            } else {
                println!("  {}: {} (found)", name, path.display());
            }
        }
        Err(_) => println!("  {}: Not found", name),
    }
}

fn check_python() {
    // Check for Python
    let python_cmd = if which::which("python3").is_ok() {
        "python3"
    } else if which::which("python").is_ok() {
        "python"
    } else {
        println!("  Python: Not found");
        return;
    };

    if let Ok(output) = std::process::Command::new(python_cmd)
        .args(["--version"])
        .output()
    {
        let version = String::from_utf8_lossy(&output.stdout);
        println!("  Python: {}", version.trim());
    }

    // Check for RealESRGAN
    if let Ok(output) = std::process::Command::new(python_cmd)
        .args(["-c", "import realesrgan; print('available')"])
        .output()
    {
        if output.status.success() {
            println!("  RealESRGAN: Available");
        } else {
            println!("  RealESRGAN: Not installed");
        }
    }

    // Check for YomiToku
    if let Ok(output) = std::process::Command::new(python_cmd)
        .args(["-c", "import yomitoku; print('available')"])
        .output()
    {
        if output.status.success() {
            println!("  YomiToku: Available");
        } else {
            println!("  YomiToku: Not installed");
        }
    }
}

// ============ Cache Info Command ============

fn run_cache_info(args: &CacheInfoArgs) -> Result<(), Box<dyn std::error::Error>> {
    use chrono::{DateTime, Local, TimeZone};

    let output_path = &args.output_pdf;

    if !output_path.exists() {
        return Err(format!("Output file not found: {}", output_path.display()).into());
    }

    match ProcessingCache::load(output_path) {
        Ok(cache) => {
            println!("=== Cache Information ===");
            println!();
            println!("Output file: {}", output_path.display());
            println!("Cache file:  {}", ProcessingCache::cache_path(output_path).display());
            println!();
            println!("Cache Version: {}", cache.version);
            let processed_dt: DateTime<Local> = Local
                .timestamp_opt(cache.processed_at as i64, 0)
                .single()
                .unwrap_or_else(Local::now);
            println!("Processed at:  {}", processed_dt.format("%Y-%m-%d %H:%M:%S"));
            println!();
            println!("Source Digest:");
            println!("  Modified: {}", cache.digest.source_modified);
            println!("  Size:     {} bytes", cache.digest.source_size);
            println!("  Options:  {}", cache.digest.options_hash);
            println!();
            println!("Processing Result:");
            println!("  Page count:  {}", cache.result.page_count);
            println!(
                "  Page shift:  {}",
                cache.result.page_number_shift
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
            println!("  Vertical:    {}", if cache.result.is_vertical { "yes" } else { "no" });
            println!("  Elapsed:     {:.2}s", cache.result.elapsed_seconds);
            println!(
                "  Output size: {} bytes ({:.2} MB)",
                cache.result.output_size,
                cache.result.output_size as f64 / 1_048_576.0
            );
        }
        Err(e) => {
            println!("No cache found for: {}", output_path.display());
            println!("Cache file would be: {}", ProcessingCache::cache_path(output_path).display());
            println!();
            println!("Reason: {}", e);
        }
    }

    Ok(())
}

// ============ Reprocess Command ============

fn run_reprocess(args: &ReprocessArgs) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Instant::now();
    let verbose = args.verbose > 0;

    // Determine if input is a state file or PDF
    let state_path = if args.is_state_file() {
        args.input.clone()
    } else {
        // For PDF input, look for state file in output directory
        let output_dir = args.output.clone().unwrap_or_else(|| {
            args.input.parent().unwrap_or(std::path::Path::new(".")).join("output")
        });
        output_dir.join(".superbook-state.json")
    };

    // Load or create state
    let mut state = if state_path.exists() {
        ReprocessState::load(&state_path)?
    } else {
        if args.is_state_file() {
            return Err(format!("State file not found: {}", state_path.display()).into());
        }
        // No existing state - need to run initial processing first
        return Err("No processing state found. Please run 'convert' command first to create initial state.".into());
    };

    // Status-only mode
    if args.status {
        print_reprocess_status(&state);
        return Ok(());
    }

    // Get failed pages to reprocess
    let failed_pages = if !args.page_indices().is_empty() {
        args.page_indices()
    } else {
        state.failed_pages()
    };

    if failed_pages.is_empty() {
        println!("No failed pages to reprocess.");
        println!("Completion: {:.1}%", state.completion_percent());
        return Ok(());
    }

    if verbose {
        println!("Reprocessing {} failed page(s)...", failed_pages.len());
        println!("Pages: {:?}", failed_pages);
    }

    // Create reprocess options
    let options = ReprocessOptions {
        max_retries: args.max_retries,
        page_indices: args.page_indices(),
        force: args.force,
        keep_intermediates: args.keep_intermediates,
    };

    // Track results
    let success_count = 0usize;
    let mut still_failed = 0usize;

    // Process each failed page
    for &page_idx in &failed_pages {
        if page_idx >= state.pages.len() {
            eprintln!("Warning: Page index {} out of range (total: {})", page_idx, state.pages.len());
            continue;
        }

        // Check retry count
        if let PageStatus::Failed { retry_count, .. } = &state.pages[page_idx] {
            if *retry_count >= options.max_retries && !args.force {
                if verbose {
                    println!("  Page {}: Skipped (max retries {} exceeded)", page_idx, options.max_retries);
                }
                still_failed += 1;
                continue;
            }
        }

        if verbose {
            println!("  Processing page {}...", page_idx);
        }

        // Note: Actual reprocessing would require pipeline integration
        // For now, we increment retry count and leave as failed
        // This is a placeholder for full pipeline integration
        if let PageStatus::Failed { error, retry_count } = &state.pages[page_idx] {
            state.pages[page_idx] = PageStatus::Failed {
                error: error.clone(),
                retry_count: retry_count + 1,
            };
            still_failed += 1;

            // In a full implementation, we would:
            // 1. Load the pipeline with same config
            // 2. Re-extract the specific page
            // 3. Run through deskew, margin, upscale, etc.
            // 4. Update state to Success or Failed
        }
    }

    // Update state timestamps
    state.updated_at = chrono::Utc::now().to_rfc3339();

    // Save state
    state.save(&state_path)?;

    let elapsed = start_time.elapsed();

    // Print summary
    if !args.quiet {
        println!();
        println!("=== Reprocess Summary ===");
        println!("Total pages:      {}", state.pages.len());
        println!("Reprocessed:      {}", failed_pages.len());
        println!("Now successful:   {}", success_count);
        println!("Still failing:    {}", still_failed);
        println!("Completion:       {:.1}%", state.completion_percent());
        println!("Time elapsed:     {:.2}s", elapsed.as_secs_f64());

        if state.is_complete() {
            println!();
            println!("All pages processed successfully!");
        } else {
            let remaining_failed = state.failed_pages();
            if !remaining_failed.is_empty() {
                println!();
                println!("Remaining failed pages: {:?}", remaining_failed);
            }
        }
    }

    Ok(())
}

fn print_reprocess_status(state: &ReprocessState) {
    println!("=== Reprocess Status ===");
    println!();
    println!("Source PDF:   {}", state.source_pdf.display());
    println!("Output dir:   {}", state.output_dir.display());
    println!("Config hash:  {}", state.config_hash);
    println!("Created:      {}", state.created_at);
    println!("Updated:      {}", state.updated_at);
    println!();
    println!("Pages: {} total", state.pages.len());

    let success_count = state.success_pages().len();
    let failed_pages = state.failed_pages();
    let pending_count = state.pages.iter().filter(|p| matches!(p, PageStatus::Pending)).count();

    println!("  Success: {}", success_count);
    println!("  Failed:  {}", failed_pages.len());
    println!("  Pending: {}", pending_count);
    println!();
    println!("Completion: {:.1}%", state.completion_percent());

    if !failed_pages.is_empty() {
        println!();
        println!("Failed pages:");
        for idx in &failed_pages {
            if let PageStatus::Failed { error, retry_count } = &state.pages[*idx] {
                println!("  Page {}: {} (retries: {})", idx, error, retry_count);
            }
        }
    }
}

// ============ Markdown Command ============

fn run_markdown(args: &MarkdownArgs) -> Result<(), Box<dyn std::error::Error>> {
    use superbook_pdf::markdown::{
        MarkdownConverter, MarkdownOptions, TextDirectionOption,
    };

    let start_time = Instant::now();

    // Validate input
    if !args.input.exists() {
        return Err(format!("Input file not found: {}", args.input.display()).into());
    }

    if !args.quiet {
        println!("PDF to Markdown Conversion");
        println!("==========================");
        println!("Input:  {}", args.input.display());
        println!("Output: {}", args.output.display());
        println!();
    }

    // Build options
    let text_direction = match args.text_direction {
        superbook_pdf::TextDirectionCli::Auto => TextDirectionOption::Auto,
        superbook_pdf::TextDirectionCli::Horizontal => TextDirectionOption::Horizontal,
        superbook_pdf::TextDirectionCli::Vertical => TextDirectionOption::Vertical,
    };

    let api_provider = if args.validate {
        Some(match args.api_provider {
            superbook_pdf::ValidationProviderCli::Claude => "claude".to_string(),
            superbook_pdf::ValidationProviderCli::Openai => "openai".to_string(),
            superbook_pdf::ValidationProviderCli::Local => "local".to_string(),
        })
    } else {
        None
    };

    let options = MarkdownOptions::builder()
        .text_direction(text_direction)
        .extract_images(args.effective_extract_images())
        .detect_tables(args.effective_detect_tables())
        .include_page_numbers(args.include_page_numbers)
        .generate_metadata(args.generate_metadata)
        .validate(args.validate)
        .api_provider_opt(api_provider)
        .build();

    // Create converter and run
    let converter = MarkdownConverter::with_options(options);
    let result = converter.convert(&args.input, &args.output)?;

    let elapsed = start_time.elapsed();

    // Print summary
    if !args.quiet {
        println!("=== Conversion Summary ===");
        println!("Pages processed:  {}", result.pages_processed);
        println!("Text blocks:      {}", result.total_blocks);
        println!("Output file:      {}", result.output_path.display());

        if !result.extracted_images.is_empty() {
            println!("Images extracted: {}", result.extracted_images.len());
        }

        if let Some(meta_path) = &result.metadata_path {
            println!("Metadata:         {}", meta_path.display());
        }

        if let Some(validation) = &result.validation {
            println!();
            println!("Validation:");
            println!("  Valid:      {}", if validation.valid { "Yes" } else { "No" });
            println!("  Confidence: {:.1}%", validation.confidence * 100.0);
            if !validation.issues.is_empty() {
                println!("  Issues:     {}", validation.issues.len());
            }
        }

        println!();
        println!("Time elapsed: {:.2}s", elapsed.as_secs_f64());
    }

    Ok(())
}

// ============ Serve Command (Web Server) ============

#[cfg(feature = "web")]
fn run_serve(args: &ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = ServerConfig::default()
        .with_port(args.port)
        .with_bind(&args.bind)
        .with_upload_limit(args.upload_limit * 1024 * 1024);

    // Configure CORS
    if args.no_cors {
        config = config.with_cors_disabled();
    } else if !args.cors_origins.is_empty() {
        config = config.with_cors_origins(args.cors_origins.clone());
    }
    // Default: permissive CORS is already set in ServerConfig::default()

    // Create tokio runtime and run the server
    // Note: WebServer must be created inside async context because WorkerPool spawns tasks
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let server = WebServer::with_config(config);
        server.run().await.map_err(|e| e.to_string())
    })?;

    Ok(())
}

// ============ Unit Tests ============

#[cfg(test)]
mod tests {
    use super::VerboseProgress;

    // TC-CLI-OUTPUT-001: DEBUG messages should only appear at verbose level 3 (-vvv)
    #[test]
    fn test_debug_messages_require_level_3() {
        // Level 0: no debug
        let handler_0 = VerboseProgress::new(0);
        assert!(!handler_0.should_show_debug());

        // Level 1 (-v): no debug
        let handler_1 = VerboseProgress::new(1);
        assert!(!handler_1.should_show_debug());

        // Level 2 (-vv): no debug
        let handler_2 = VerboseProgress::new(2);
        assert!(!handler_2.should_show_debug());

        // Level 3 (-vvv): should show debug
        let handler_3 = VerboseProgress::new(3);
        assert!(handler_3.should_show_debug());
    }

    // TC-CLI-OUTPUT-002: Verbose level thresholds
    #[test]
    fn test_verbose_level_thresholds() {
        // Level 0: no output
        let handler_0 = VerboseProgress::new(0);
        assert!(!handler_0.should_show_steps());
        assert!(!handler_0.should_show_progress());
        assert!(!handler_0.should_show_debug());

        // Level 1 (-v): basic progress
        let handler_1 = VerboseProgress::new(1);
        assert!(handler_1.should_show_steps());
        assert!(handler_1.should_show_progress());
        assert!(!handler_1.should_show_debug());

        // Level 2 (-vv): detailed info
        let handler_2 = VerboseProgress::new(2);
        assert!(handler_2.should_show_steps());
        assert!(handler_2.should_show_progress());
        assert!(!handler_2.should_show_debug());

        // Level 3 (-vvv): debug info
        let handler_3 = VerboseProgress::new(3);
        assert!(handler_3.should_show_steps());
        assert!(handler_3.should_show_progress());
        assert!(handler_3.should_show_debug());
    }
}
