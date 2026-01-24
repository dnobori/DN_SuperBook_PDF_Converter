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
    CacheInfoArgs, Cli, Commands, ConvertArgs,
    // Config
    CliOverrides, Config,
    // Pipeline
    PdfPipeline, ProgressCallback,
    // Progress tracking
    ProgressTracker,
};

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Convert(args) => run_convert(&args),
        Commands::Info => run_info(),
        Commands::CacheInfo(args) => run_cache_info(&args),
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
        if self.verbose_level > 1 {
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
    println!("System Information:");
    println!("  Platform: {}", std::env::consts::OS);
    println!("  Arch: {}", std::env::consts::ARCH);
    println!("  CPUs: {}", num_cpus::get());

    println!();
    println!("External Tools:");
    check_tool("magick", "ImageMagick");
    check_tool("pdftoppm", "Poppler (pdftoppm)");
    check_tool("gs", "Ghostscript");
    check_tool("tesseract", "Tesseract OCR");

    println!();
    println!("GPU Status:");
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name,memory.total")
        .arg("--format=csv,noheader")
        .output()
    {
        if output.status.success() {
            let gpu_info = String::from_utf8_lossy(&output.stdout);
            println!("  NVIDIA GPU: {}", gpu_info.trim());
        } else {
            println!("  NVIDIA GPU: Not detected");
        }
    } else {
        println!("  NVIDIA GPU: nvidia-smi not found");
    }

    Ok(())
}

fn check_tool(cmd: &str, name: &str) {
    match which::which(cmd) {
        Ok(path) => println!("  {}: {} (found)", name, path.display()),
        Err(_) => println!("  {}: Not found", name),
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
