//! superbook-pdf - High-quality PDF converter for scanned books
//!
//! CLI entry point

use clap::Parser;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use superbook_pdf::pdf_writer::{OcrLayer, OcrPageText, TextBlock};
use superbook_pdf::{
    exit_codes,
    AiBridgeConfig,
    Cli,
    // Phase 1-6: Advanced processing modules
    ColorAnalyzer,
    Commands,
    ConvertArgs,
    DeskewOptions,
    ExtractOptions,
    FinalizeOptions,
    GroupCropAnalyzer,
    ImageMarginDetector,
    ImageNormalizer,
    ImageProcDeskewer,
    LopdfExtractor,
    LopdfReader,
    MarginOptions,
    NormalizeOptions,
    PageFinalizer,
    PageOffsetAnalyzer,
    PdfWriterOptions,
    PrintPdfWriter,
    RealEsrgan,
    RealEsrganOptions,
    SubprocessBridge,
    TesseractPageDetector,
    // v0.2.0: Vertical text detection
    VerticalDetectOptions,
    YomiToku,
    YomiTokuOptions,
    detect_book_vertical_writing,
};

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Convert(args) => run_convert(&args),
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

    if args.dry_run {
        print_execution_plan(args, &pdf_files);
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

        process_single_pdf(pdf_path, args)?;
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
    if args.effective_internal_resolution() {
        println!("  6. Internal Resolution Normalization (4960x7016): ENABLED");
    }
    if args.effective_color_correction() {
        println!("  7. Global Color Correction: ENABLED");
    }
    if args.effective_offset_alignment() {
        println!("  8. Page Number Offset Alignment: ENABLED");
    }
    println!(
        "  9. PDF Generation (output height: {})",
        args.output_height
    );
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

    let extracted_pages = LopdfExtractor::extract_auto(pdf_path, &extracted_dir, &extract_options)?;
    if verbose {
        println!("    Extracted {} pages", extracted_pages.len());
    }

    // Step 3: Deskew (if enabled) - parallel
    let deskewed_dir = work_dir.join("deskewed");
    let images_after_deskew: Vec<PathBuf> = if args.effective_deskew() {
        if verbose {
            println!("  Applying deskew correction...");
        }
        std::fs::create_dir_all(&deskewed_dir)?;

        let deskew_options = DeskewOptions::default();

        // Prepare output paths
        let output_paths: Vec<PathBuf> = extracted_pages
            .iter()
            .map(|page| deskewed_dir.join(page.path.file_name().unwrap()))
            .collect();

        // Deskew in parallel
        let deskewed_images: Vec<PathBuf> = extracted_pages
            .par_iter()
            .zip(output_paths.par_iter())
            .map(|(page, output_path)| {
                match ImageProcDeskewer::correct_skew(&page.path, output_path, &deskew_options) {
                    Ok(result) => {
                        if verbose && args.verbose > 1 && result.detection.angle.abs() > 0.1 {
                            eprintln!(
                                "\n    Page {}: corrected {:.2}°",
                                page.page_index, result.detection.angle
                            );
                        }
                    }
                    Err(e) => {
                        if verbose && args.verbose > 1 {
                            eprintln!("\n    Page {}: deskew failed ({})", page.page_index, e);
                        }
                        // Copy original on failure
                        std::fs::copy(&page.path, output_path).ok();
                    }
                }
                output_path.clone()
            })
            .collect();

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

                // Trim all images in parallel
                let output_paths: Vec<PathBuf> = images_after_deskew
                    .iter()
                    .map(|img_path| trimmed_dir.join(img_path.file_name().unwrap()))
                    .collect();

                let trimmed_images: Vec<PathBuf> = images_after_deskew
                    .par_iter()
                    .zip(output_paths.par_iter())
                    .enumerate()
                    .map(|(i, (img_path, output_path))| {
                        match ImageMarginDetector::trim(img_path, output_path, &unified.margins) {
                            Ok(result) => {
                                if verbose && args.verbose > 1 {
                                    eprintln!(
                                        "\n    Page {}: {}x{} -> {}x{}",
                                        i,
                                        result.original_size.0,
                                        result.original_size.1,
                                        result.trimmed_size.0,
                                        result.trimmed_size.1
                                    );
                                }
                            }
                            Err(e) => {
                                if verbose && args.verbose > 1 {
                                    eprintln!(
                                        "\n    Page {}: trim failed ({}), keeping original",
                                        i, e
                                    );
                                }
                                std::fs::copy(img_path, output_path).ok();
                            }
                        }
                        output_path.clone()
                    })
                    .collect();

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

    // === Phase 6: Advanced Processing Pipeline ===

    // Step 6: Internal Resolution Normalization (if enabled)
    let normalized_dir = work_dir.join("normalized");
    let images_after_normalize: Vec<PathBuf> = if args.effective_internal_resolution() {
        if verbose {
            println!("  Normalizing to internal resolution (4960x7016)...");
        }
        std::fs::create_dir_all(&normalized_dir)?;

        let normalize_options = NormalizeOptions::builder()
            .target_width(4960)
            .target_height(7016)
            .build();

        // Prepare output paths
        let output_paths: Vec<PathBuf> = (0..images_for_pdf.len())
            .map(|i| normalized_dir.join(format!("page_{:04}.png", i)))
            .collect();

        // Progress counter
        let completed = Arc::new(AtomicUsize::new(0));
        let total = images_for_pdf.len();

        // Parallel normalization
        let results: Vec<_> = images_for_pdf
            .par_iter()
            .zip(output_paths.par_iter())
            .enumerate()
            .map(|(i, (img_path, output_path))| {
                let result = ImageNormalizer::normalize(img_path, output_path, &normalize_options);

                // Update progress
                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if verbose && done.is_multiple_of(10) {
                    print!("\r    Normalizing: {}/{}", done, total);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }

                match result {
                    Ok(_) => output_path.clone(),
                    Err(e) => {
                        if verbose && args.verbose > 1 {
                            eprintln!("\n    Page {}: normalization failed ({})", i + 1, e);
                        }
                        // Copy original on failure
                        std::fs::copy(img_path, output_path).ok();
                        output_path.clone()
                    }
                }
            })
            .collect();

        if verbose {
            println!("\r    Normalized {} images    ", results.len());
        }
        results
    } else {
        images_for_pdf.clone()
    };

    // Step 7: Color Statistics Analysis and Global Color Correction (if enabled)
    let color_corrected_dir = work_dir.join("color_corrected");
    let images_after_color: Vec<PathBuf> =
        if args.effective_color_correction() {
            if verbose {
                println!("  Analyzing color statistics...");
            }
            std::fs::create_dir_all(&color_corrected_dir)?;

            // Collect color statistics from all pages (parallel)
            let stats_results: Vec<_> = images_after_normalize
                .par_iter()
                .enumerate()
                .map(|(i, img_path)| {
                    let result = ColorAnalyzer::calculate_stats(img_path);
                    (i, result)
                })
                .collect();

            // Aggregate stats (sequential - needs all results)
            let mut all_stats = Vec::new();
            for (i, result) in stats_results {
                match result {
                    Ok(stats) => {
                        if verbose && args.verbose > 1 {
                            println!(
                                "    Page {}: paper=({:.1},{:.1},{:.1}) ink=({:.1},{:.1},{:.1})",
                                i + 1,
                                stats.paper_r,
                                stats.paper_g,
                                stats.paper_b,
                                stats.ink_r,
                                stats.ink_g,
                                stats.ink_b
                            );
                        }
                        all_stats.push(stats);
                    }
                    Err(e) => {
                        if verbose {
                            println!("    Page {}: color analysis failed ({})", i + 1, e);
                        }
                    }
                }
            }

            // Calculate global color adjustment parameters
            if !all_stats.is_empty() {
                let global_param = ColorAnalyzer::decide_global_adjustment(&all_stats);
                if verbose {
                    println!(
                    "    Global adjustment: scale=({:.3},{:.3},{:.3}) offset=({:.1},{:.1},{:.1})",
                    global_param.scale_r, global_param.scale_g, global_param.scale_b,
                    global_param.offset_r, global_param.offset_g, global_param.offset_b
                );
                }

                // Apply color correction to all images (parallel)
                let output_paths: Vec<PathBuf> = (0..images_after_normalize.len())
                    .map(|i| color_corrected_dir.join(format!("page_{:04}.png", i)))
                    .collect();

                let corrected_images: Vec<PathBuf> = images_after_normalize
                    .par_iter()
                    .zip(output_paths.par_iter())
                    .enumerate()
                    .map(|(i, (img_path, output_path))| {
                        match image::open(img_path) {
                            Ok(img) => {
                                let mut rgb_img = img.to_rgb8();
                                ColorAnalyzer::apply_adjustment(&mut rgb_img, &global_param);
                                if let Err(e) = rgb_img.save(output_path) {
                                    if verbose && args.verbose > 1 {
                                        eprintln!(
                                            "\n    Page {}: save failed ({}), keeping original",
                                            i + 1,
                                            e
                                        );
                                    }
                                    std::fs::copy(img_path, output_path).ok();
                                }
                            }
                            Err(e) => {
                                if verbose && args.verbose > 1 {
                                    eprintln!(
                                        "\n    Page {}: color correction failed ({}), keeping original",
                                        i + 1,
                                        e
                                    );
                                }
                                std::fs::copy(img_path, output_path).ok();
                            }
                        }
                        output_path.clone()
                    })
                    .collect();

                if verbose {
                    println!("    Color corrected {} images", corrected_images.len());
                }
                corrected_images
            } else {
                images_after_normalize.clone()
            }
        } else {
            images_after_normalize.clone()
        };

    // Step 8: Tukey Fence Group Crop (if offset_alignment is enabled)
    let cropped_dir = work_dir.join("cropped");
    let images_after_crop: Vec<PathBuf> = if args.effective_offset_alignment() {
        if verbose {
            println!("  Detecting text bounding boxes...");
        }
        std::fs::create_dir_all(&cropped_dir)?;

        // Detect bounding boxes for all pages
        let bounding_boxes = GroupCropAnalyzer::detect_all_bounding_boxes(&images_after_color, 240);

        if !bounding_boxes.is_empty() {
            // Calculate unified crop regions using Tukey fence with Y unification and margin expansion
            // Use internal resolution bounds (4960x7016) and add 5% margin for text breathing room
            let unified = GroupCropAnalyzer::unify_and_expand_regions(
                &bounding_boxes,
                5,    // 5% margin expansion
                4960, // internal width limit
                7016, // internal height limit
            );

            if verbose && args.verbose > 1 {
                println!(
                    "    Odd pages: crop region {}x{} at ({},{}) ({} inliers)",
                    unified.odd_region.width,
                    unified.odd_region.height,
                    unified.odd_region.left,
                    unified.odd_region.top,
                    unified.odd_region.inlier_count
                );
                println!(
                    "    Even pages: crop region {}x{} at ({},{}) ({} inliers)",
                    unified.even_region.width,
                    unified.even_region.height,
                    unified.even_region.left,
                    unified.even_region.top,
                    unified.even_region.inlier_count
                );
            }

            // Apply crop to all images (parallel)
            let output_paths: Vec<PathBuf> = (0..images_after_color.len())
                .map(|i| cropped_dir.join(format!("page_{:04}.png", i)))
                .collect();

            let cropped_images: Vec<PathBuf> = images_after_color
                .par_iter()
                .zip(output_paths.par_iter())
                .enumerate()
                .map(|(i, (img_path, output_path))| {
                    let region = if i % 2 == 0 {
                        &unified.odd_region
                    } else {
                        &unified.even_region
                    };

                    // Use the crop region to trim the image
                    match image::open(img_path) {
                        Ok(img) => {
                            let cropped = img.crop_imm(
                                region.left,
                                region.top,
                                region.width.min(img.width() - region.left),
                                region.height.min(img.height() - region.top),
                            );
                            if let Err(e) = cropped.save(output_path) {
                                if verbose && args.verbose > 1 {
                                    eprintln!(
                                        "\n    Page {}: crop failed ({}), keeping original",
                                        i + 1,
                                        e
                                    );
                                }
                                std::fs::copy(img_path, output_path).ok();
                            }
                        }
                        Err(e) => {
                            if verbose && args.verbose > 1 {
                                eprintln!(
                                    "\n    Page {}: image load failed ({}), keeping original",
                                    i + 1,
                                    e
                                );
                            }
                            std::fs::copy(img_path, output_path).ok();
                        }
                    }
                    output_path.clone()
                })
                .collect();

            if verbose {
                println!("    Cropped {} images", cropped_images.len());
            }
            cropped_images
        } else {
            images_after_color.clone()
        }
    } else {
        images_after_color.clone()
    };

    // Step 9: Page Number Offset Calculation (if offset_alignment is enabled)
    let _offset_analysis = if args.effective_offset_alignment() {
        if verbose {
            println!("  Detecting page numbers and calculating offsets...");
        }

        // Detect page numbers using Tesseract
        let page_options = superbook_pdf::PageNumberOptions::default();
        let mut page_detections = Vec::new();
        for (i, img_path) in images_after_crop.iter().enumerate() {
            match TesseractPageDetector::detect_single(img_path, i, &page_options) {
                Ok(detection) => {
                    if let Some(num) = detection.number {
                        if verbose && args.verbose > 1 {
                            println!("    Page {}: detected number {}", i + 1, num);
                        }
                    }
                    page_detections.push(detection);
                }
                Err(e) => {
                    if verbose && args.verbose > 1 {
                        println!("    Page {}: page number detection failed ({})", i + 1, e);
                    }
                }
            }
        }

        // Analyze offsets
        if !page_detections.is_empty() {
            let first_img = image::open(&images_after_crop[0]).ok();
            let img_height = first_img.as_ref().map(|img| img.height()).unwrap_or(7016);

            let analysis = PageOffsetAnalyzer::analyze_offsets(&page_detections, img_height);

            if verbose {
                println!(
                    "    Page number shift: {}, confidence: {:.1}%",
                    analysis.page_number_shift,
                    analysis.confidence * 100.0
                );
                if let Some(odd_x) = analysis.odd_avg_x {
                    println!("    Odd pages avg X offset: {}", odd_x);
                }
                if let Some(even_x) = analysis.even_avg_x {
                    println!("    Even pages avg X offset: {}", even_x);
                }
            }

            Some(analysis)
        } else {
            None
        }
    } else {
        None
    };

    // Step 10: Final Output (resize with gradient padding) - parallel
    let finalized_dir = work_dir.join("finalized");
    let final_images: Vec<PathBuf> = if args.output_height != 0 && args.output_height != 7016 {
        if verbose {
            println!("  Finalizing output (height: {})...", args.output_height);
        }
        std::fs::create_dir_all(&finalized_dir)?;

        let finalize_options = FinalizeOptions::builder()
            .target_height(args.output_height)
            .build();

        // Prepare output paths
        let output_paths: Vec<PathBuf> = (0..images_after_crop.len())
            .map(|i| finalized_dir.join(format!("page_{:04}.png", i)))
            .collect();

        // Finalize all images in parallel
        let finalized_images: Vec<PathBuf> = images_after_crop
            .par_iter()
            .zip(output_paths.par_iter())
            .enumerate()
            .map(|(i, (img_path, output_path))| {
                // finalize takes 6 args: input, output, options, crop_region, shift_x, shift_y
                match PageFinalizer::finalize(img_path, output_path, &finalize_options, None, 0, 0)
                {
                    Ok(_) => {}
                    Err(e) => {
                        if verbose && args.verbose > 1 {
                            eprintln!(
                                "\n    Page {}: finalize failed ({}), keeping original",
                                i + 1,
                                e
                            );
                        }
                        std::fs::copy(img_path, output_path).ok();
                    }
                }
                output_path.clone()
            })
            .collect();

        if verbose {
            println!("    Finalized {} images", finalized_images.len());
        }
        finalized_images
    } else {
        images_after_crop.clone()
    };

    // Step 10.5: Vertical text detection (Japanese books)
    let is_vertical_writing = if args.advanced && !final_images.is_empty() {
        if verbose {
            println!("  Detecting text direction...");
        }

        // Load images as grayscale for vertical detection
        let mut gray_images = Vec::new();
        for img_path in final_images.iter().take(20) {
            // Sample up to 20 pages
            if let Ok(img) = image::open(img_path) {
                gray_images.push(img.to_luma8());
            }
        }

        if !gray_images.is_empty() {
            let vd_options = VerticalDetectOptions::default();
            match detect_book_vertical_writing(&gray_images, &vd_options) {
                Ok(result) => {
                    if verbose {
                        println!(
                            "    Text direction: {} (probability: {:.1}%)",
                            if result.is_vertical {
                                "vertical"
                            } else {
                                "horizontal"
                            },
                            result.vertical_probability * 100.0
                        );
                    }
                    result.is_vertical
                }
                Err(e) => {
                    if verbose && args.verbose > 1 {
                        println!("    Vertical detection failed: {}", e);
                    }
                    false
                }
            }
        } else {
            false
        }
    } else {
        false
    };

    // Note: is_vertical_writing will be used for PDF viewer preferences in future versions
    let _ = is_vertical_writing;

    // Step 11: OCR with YomiToku (if enabled)
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
                for (i, img_path) in final_images.iter().enumerate() {
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
                    println!(
                        "    OCR completed: {}/{} pages",
                        success_count,
                        results.len()
                    );
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

    // Step 12: Generate output PDF
    if verbose {
        println!("  Generating output PDF...");
    }
    let output_pdf = args.output.join(format!("{}_converted.pdf", pdf_name));

    // Convert OCR results to OcrLayer
    let ocr_layer = if !ocr_results.is_empty() {
        let pages: Vec<OcrPageText> = ocr_results
            .iter()
            .enumerate()
            .filter_map(|(idx, result)| {
                result.as_ref().map(|r| OcrPageText {
                    page_index: idx,
                    blocks: r
                        .text_blocks
                        .iter()
                        .map(|b| TextBlock {
                            x: b.bbox.0 as f64,
                            y: b.bbox.1 as f64,
                            width: (b.bbox.2 - b.bbox.0) as f64,
                            height: (b.bbox.3 - b.bbox.1) as f64,
                            text: b.text.clone(),
                            font_size: b.font_size.unwrap_or(12.0) as f64,
                            vertical: matches!(b.direction, superbook_pdf::TextDirection::Vertical),
                        })
                        .collect(),
                })
            })
            .collect();

        if pages.is_empty() {
            None
        } else {
            Some(OcrLayer { pages })
        }
    } else {
        None
    };

    let mut pdf_builder = PdfWriterOptions::builder()
        .dpi(args.dpi)
        .jpeg_quality(args.jpeg_quality)
        .metadata(reader.info.metadata);

    if let Some(layer) = ocr_layer {
        pdf_builder = pdf_builder.ocr_layer(layer);
    }

    let pdf_options = pdf_builder.build();

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
    check_tool("pdftoppm", "Poppler (pdftoppm)");
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
