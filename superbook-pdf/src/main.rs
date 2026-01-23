//! superbook-pdf - High-quality PDF converter for scanned books
//!
//! CLI entry point

use clap::Parser;
use std::path::PathBuf;
use std::time::Instant;
use superbook_pdf::{
    exit_codes, AiBridgeConfig, Cli, Commands, ConvertArgs, DeskewOptions, ExtractOptions,
    ImageMarginDetector, ImageProcDeskewer, LopdfReader, MagickExtractor, MarginOptions,
    PdfWriterOptions, PrintPdfWriter, RealEsrgan, RealEsrganOptions, SubprocessBridge,
    YomiToku, YomiTokuOptions,
};

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Convert(args) => run_convert(args),
        Commands::Info => run_info(),
    };

    std::process::exit(match result {
        Ok(()) => exit_codes::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            exit_codes::GENERAL_ERROR
        }
    });
}

fn run_convert(args: ConvertArgs) -> Result<(), Box<dyn std::error::Error>> {
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

    if args.dry_run {
        print_execution_plan(&args, &pdf_files);
        return Ok(());
    }

    // Create output directory
    std::fs::create_dir_all(&args.output)?;

    let verbose = args.verbose > 0;

    // Process each PDF file
    for (idx, pdf_path) in pdf_files.iter().enumerate() {
        if verbose {
            println!(
                "[{}/{}] Processing: {}",
                idx + 1,
                pdf_files.len(),
                pdf_path.display()
            );
        }

        process_single_pdf(pdf_path, &args)?;
    }

    let elapsed = start_time.elapsed();
    if !args.quiet {
        println!(
            "Completed {} file(s) in {:.2}s",
            pdf_files.len(),
            elapsed.as_secs_f64()
        );
    }

    Ok(())
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
fn print_execution_plan(args: &ConvertArgs, pdf_files: &[PathBuf]) {
    println!("=== Dry Run - Execution Plan ===");
    println!();
    println!("Input: {}", args.input.display());
    println!("Output: {}", args.output.display());
    println!("Files to process: {}", pdf_files.len());
    println!();
    println!("Pipeline Configuration:");
    println!("  1. Image Extraction (DPI: {})", args.dpi);
    if args.effective_deskew() {
        println!("  2. Deskew Correction: ENABLED");
    } else {
        println!("  2. Deskew Correction: DISABLED");
    }
    println!("  3. Margin Trim: {}%", args.margin_trim);
    if args.effective_upscale() {
        println!("  4. AI Upscaling (RealESRGAN 2x): ENABLED");
    } else {
        println!("  4. AI Upscaling: DISABLED");
    }
    if args.ocr {
        println!("  5. OCR (YomiToku): ENABLED");
    } else {
        println!("  5. OCR: DISABLED");
    }
    println!("  6. PDF Generation");
    println!();
    println!("Processing Options:");
    println!("  Threads: {}", args.thread_count());
    println!("  GPU: {}", if args.effective_gpu() { "YES" } else { "NO" });
    println!("  Verbose: {}", args.verbose);
    println!();
    println!("Files:");
    for (i, file) in pdf_files.iter().enumerate() {
        println!("  {}. {}", i + 1, file.display());
    }
}

/// Process a single PDF file through the conversion pipeline
fn process_single_pdf(
    pdf_path: &PathBuf,
    args: &ConvertArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let verbose = args.verbose > 0;

    // Step 1: Read PDF metadata
    if verbose {
        println!("  Reading PDF...");
    }
    let reader = LopdfReader::new(pdf_path)?;
    let page_count = reader.info.page_count;
    if verbose {
        println!("    Pages: {}", page_count);
    }

    // Create working directory for this PDF
    let pdf_name = pdf_path.file_stem().unwrap_or_default().to_string_lossy();
    let work_dir = args.output.join(format!(".work_{}", pdf_name));
    std::fs::create_dir_all(&work_dir)?;

    // Step 2: Extract images
    if verbose {
        println!("  Extracting images (DPI: {})...", args.dpi);
    }
    let extract_options = ExtractOptions::builder().dpi(args.dpi).build();

    let extracted_dir = work_dir.join("extracted");
    std::fs::create_dir_all(&extracted_dir)?;

    let extracted_pages = MagickExtractor::extract_all(pdf_path, &extracted_dir, &extract_options)?;
    if verbose {
        println!("    Extracted {} pages", extracted_pages.len());
    }

    // Step 3: Deskew (if enabled)
    let deskewed_dir = work_dir.join("deskewed");
    let images_after_deskew: Vec<PathBuf> = if args.effective_deskew() {
        if verbose {
            println!("  Applying deskew correction...");
        }
        std::fs::create_dir_all(&deskewed_dir)?;

        let deskew_options = DeskewOptions::default();
        let mut deskewed_images = Vec::new();

        for page in &extracted_pages {
            let output_path = deskewed_dir.join(page.path.file_name().unwrap());
            let result =
                ImageProcDeskewer::correct_skew(&page.path, &output_path, &deskew_options)?;

            if verbose && args.verbose > 1 && result.detection.angle.abs() > 0.1 {
                println!(
                    "    Page {}: corrected {:.2}°",
                    page.page_index, result.detection.angle
                );
            }

            deskewed_images.push(output_path);
        }

        deskewed_images
    } else {
        extracted_pages.iter().map(|p| p.path.clone()).collect()
    };

    // Step 4: Margin Trimming (if margin_trim > 0)
    let trimmed_dir = work_dir.join("trimmed");
    let images_after_trim: Vec<PathBuf> = if args.margin_trim > 0.0 {
        if verbose {
            println!("  Trimming margins ({}%)...", args.margin_trim);
        }
        std::fs::create_dir_all(&trimmed_dir)?;

        let margin_options = MarginOptions::builder()
            .default_trim_percent(args.margin_trim)
            .build();

        // First detect unified margins across all pages
        match ImageMarginDetector::detect_unified(&images_after_deskew, &margin_options) {
            Ok(unified) => {
                if verbose && args.verbose > 1 {
                    println!(
                        "    Unified margins: T={} B={} L={} R={}",
                        unified.margins.top,
                        unified.margins.bottom,
                        unified.margins.left,
                        unified.margins.right
                    );
                }

                let mut trimmed_images = Vec::new();
                for (i, img_path) in images_after_deskew.iter().enumerate() {
                    let output_path = trimmed_dir.join(img_path.file_name().unwrap());
                    match ImageMarginDetector::trim(img_path, &output_path, &unified.margins) {
                        Ok(result) => {
                            if verbose && args.verbose > 1 {
                                println!(
                                    "    Page {}: {}x{} -> {}x{}",
                                    i,
                                    result.original_size.0,
                                    result.original_size.1,
                                    result.trimmed_size.0,
                                    result.trimmed_size.1
                                );
                            }
                            trimmed_images.push(output_path);
                        }
                        Err(e) => {
                            if verbose {
                                println!("    Page {}: trim failed ({}), keeping original", i, e);
                            }
                            std::fs::copy(img_path, &output_path)?;
                            trimmed_images.push(output_path);
                        }
                    }
                }
                trimmed_images
            }
            Err(e) => {
                if verbose {
                    println!(
                        "    Warning: Margin detection failed ({}), skipping trim",
                        e
                    );
                }
                images_after_deskew.clone()
            }
        }
    } else {
        images_after_deskew
    };

    // Step 5: AI Upscaling (if enabled) - requires Python bridge
    let upscaled_dir = work_dir.join("upscaled");
    let images_for_pdf = if args.effective_upscale() {
        if verbose {
            println!("  AI Upscaling (RealESRGAN)...");
        }

        // Try to initialize RealESRGAN via SubprocessBridge
        let venv_path = std::env::var("SUPERBOOK_VENV")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./venv"));

        let bridge_config = AiBridgeConfig::builder().venv_path(venv_path).build();

        match SubprocessBridge::new(bridge_config) {
            Ok(bridge) => {
                let esrgan = RealEsrgan::new(bridge);
                std::fs::create_dir_all(&upscaled_dir)?;

                let mut options = RealEsrganOptions::builder().scale(2);
                if args.effective_gpu() {
                    options = options.gpu_id(0);
                }
                let options = options.build();

                let progress_fn = if verbose {
                    Some(Box::new(|current: usize, total: usize| {
                        print!("\r    Upscaling: {}/{}", current, total);
                        use std::io::Write;
                        std::io::stdout().flush().ok();
                    }) as Box<dyn Fn(usize, usize) + Send>)
                } else {
                    None
                };

                match esrgan.upscale_batch(&images_after_trim, &upscaled_dir, &options, progress_fn)
                {
                    Ok(result) => {
                        if verbose {
                            println!();
                            println!("    Upscaled {} images", result.successful.len());
                        }
                        // Collect upscaled images in order, using the actual output paths
                        result
                            .successful
                            .iter()
                            .map(|r| r.output_path.clone())
                            .collect()
                    }
                    Err(e) => {
                        if verbose {
                            println!();
                            println!(
                                "    Warning: Upscaling failed ({}), using original images",
                                e
                            );
                        }
                        images_after_trim
                    }
                }
            }
            Err(e) => {
                if verbose {
                    println!(
                        "    Note: RealESRGAN not available ({}), skipping upscaling",
                        e
                    );
                }
                images_after_trim
            }
        }
    } else {
        images_after_trim
    };

    // Step 6: OCR with YomiToku (if enabled)
    let ocr_results = if args.ocr {
        if verbose {
            println!("  Running OCR (YomiToku)...");
        }

        // Try to initialize YomiToku via SubprocessBridge
        let venv_path = std::env::var("SUPERBOOK_VENV")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./venv"));

        let bridge_config = AiBridgeConfig::builder().venv_path(venv_path).build();

        match SubprocessBridge::new(bridge_config) {
            Ok(bridge) => {
                let yomitoku = YomiToku::new(bridge);
                let mut ocr_opts = YomiTokuOptions::builder();
                if args.effective_gpu() {
                    ocr_opts = ocr_opts.use_gpu(true).gpu_id(0);
                }
                let ocr_opts = ocr_opts.build();

                let mut results = Vec::new();
                for (i, img_path) in images_for_pdf.iter().enumerate() {
                    match yomitoku.ocr(img_path, &ocr_opts) {
                        Ok(result) => {
                            if verbose && args.verbose > 1 {
                                println!(
                                    "    Page {}: {} text blocks detected",
                                    i + 1,
                                    result.text_blocks.len()
                                );
                            }
                            results.push(Some(result));
                        }
                        Err(e) => {
                            if verbose {
                                println!("    Page {}: OCR failed ({})", i + 1, e);
                            }
                            results.push(None);
                        }
                    }
                }

                if verbose {
                    let success_count = results.iter().filter(|r| r.is_some()).count();
                    println!("    OCR completed: {}/{} pages", success_count, results.len());
                }
                results
            }
            Err(e) => {
                if verbose {
                    println!("    Note: YomiToku not available ({}), skipping OCR", e);
                }
                vec![]
            }
        }
    } else {
        vec![]
    };

    // Step 7: Generate output PDF
    if verbose {
        println!("  Generating output PDF...");
    }
    let output_pdf = args.output.join(format!("{}_converted.pdf", pdf_name));

    let pdf_options = PdfWriterOptions::builder()
        .dpi(args.dpi)
        .metadata(reader.info.metadata.clone())
        .build();

    // TODO: Pass OCR results to PDF writer when OCR layer support is fully implemented
    let _ = ocr_results; // Suppress unused warning for now

    PrintPdfWriter::create_from_images(&images_for_pdf, &output_pdf, &pdf_options)?;

    if verbose {
        println!("    Output: {}", output_pdf.display());
    }

    // Cleanup working directory
    if !args.quiet && args.verbose < 2 {
        std::fs::remove_dir_all(&work_dir)?;
    }

    Ok(())
}

fn run_info() -> Result<(), Box<dyn std::error::Error>> {
    println!("superbook-pdf v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("System Information:");
    println!("  Platform: {}", std::env::consts::OS);
    println!("  Arch: {}", std::env::consts::ARCH);
    println!("  CPUs: {}", num_cpus::get());

    // Check for external tools
    println!();
    println!("External Tools:");
    check_tool("magick", "ImageMagick");
    check_tool("gs", "Ghostscript");
    check_tool("tesseract", "Tesseract OCR");

    // Check for GPU
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
