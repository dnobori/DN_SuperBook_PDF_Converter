//! Benchmarks for the superbook-pdf processing pipeline
//!
//! Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use superbook_pdf::{
    DeskewOptions, ExtractOptions, MarginOptions, PdfWriterOptions, YomiTokuOptions,
};

/// Benchmark option builder construction
fn bench_option_builders(c: &mut Criterion) {
    let mut group = c.benchmark_group("option_builders");

    group.bench_function("ExtractOptions::builder", |b| {
        b.iter(|| {
            black_box(
                ExtractOptions::builder()
                    .dpi(300)
                    .format(superbook_pdf::image_extract::ImageFormat::Png)
                    .build(),
            )
        })
    });

    group.bench_function("DeskewOptions::builder", |b| {
        b.iter(|| {
            black_box(
                DeskewOptions::builder()
                    .max_angle(15.0)
                    .threshold_angle(0.5)
                    .build(),
            )
        })
    });

    group.bench_function("MarginOptions::builder", |b| {
        b.iter(|| black_box(MarginOptions::builder().default_trim_percent(1.0).build()))
    });

    group.bench_function("PdfWriterOptions::builder", |b| {
        b.iter(|| {
            black_box(
                PdfWriterOptions::builder()
                    .dpi(300)
                    .jpeg_quality(90)
                    .build(),
            )
        })
    });

    group.bench_function("YomiTokuOptions::builder", |b| {
        b.iter(|| {
            black_box(
                YomiTokuOptions::builder()
                    .use_gpu(true)
                    .confidence_threshold(0.5)
                    .build(),
            )
        })
    });

    group.finish();
}

/// Benchmark preset creation
fn bench_presets(c: &mut Criterion) {
    let mut group = c.benchmark_group("presets");

    group.bench_function("ExtractOptions::high_quality", |b| {
        b.iter(|| black_box(ExtractOptions::high_quality()))
    });

    group.bench_function("ExtractOptions::fast", |b| {
        b.iter(|| black_box(ExtractOptions::fast()))
    });

    group.bench_function("DeskewOptions::high_quality", |b| {
        b.iter(|| black_box(DeskewOptions::high_quality()))
    });

    group.bench_function("DeskewOptions::fast", |b| {
        b.iter(|| black_box(DeskewOptions::fast()))
    });

    group.bench_function("PdfWriterOptions::high_quality", |b| {
        b.iter(|| black_box(PdfWriterOptions::high_quality()))
    });

    group.bench_function("PdfWriterOptions::compact", |b| {
        b.iter(|| black_box(PdfWriterOptions::compact()))
    });

    group.finish();
}

/// Benchmark utility functions
fn bench_utilities(c: &mut Criterion) {
    let mut group = c.benchmark_group("utilities");

    // Conversion functions
    group.bench_function("mm_to_points", |b| {
        b.iter(|| black_box(superbook_pdf::mm_to_points(210.0)))
    });

    group.bench_function("points_to_mm", |b| {
        b.iter(|| black_box(superbook_pdf::points_to_mm(595.0)))
    });

    group.bench_function("mm_to_pixels", |b| {
        b.iter(|| black_box(superbook_pdf::mm_to_pixels(210.0, 300)))
    });

    group.bench_function("pixels_to_mm", |b| {
        b.iter(|| black_box(superbook_pdf::pixels_to_mm(2480, 300)))
    });

    // Clamping
    group.bench_function("clamp", |b| {
        b.iter(|| black_box(superbook_pdf::clamp(150, 0, 100)))
    });

    // Percentage calculation
    group.bench_function("percentage", |b| {
        b.iter(|| black_box(superbook_pdf::percentage(75, 100)))
    });

    group.finish();
}

/// Benchmark file size formatting
fn bench_format_functions(c: &mut Criterion) {
    let mut group = c.benchmark_group("format_functions");

    let sizes = [1024u64, 1024 * 1024, 1024 * 1024 * 1024];
    for size in sizes {
        group.bench_with_input(
            BenchmarkId::new("format_file_size", size),
            &size,
            |b, &size| b.iter(|| black_box(superbook_pdf::format_file_size(size))),
        );
    }

    let durations = [1.5, 60.0, 3600.0];
    for dur in durations {
        group.bench_with_input(
            BenchmarkId::new("format_duration", format!("{:.0}s", dur)),
            &dur,
            |b, &dur| {
                let d = std::time::Duration::from_secs_f64(dur);
                b.iter(|| black_box(superbook_pdf::format_duration(d)))
            },
        );
    }

    group.finish();
}

/// Benchmark deskew angle detection (synthetic data)
fn bench_deskew_detection(c: &mut Criterion) {
    use superbook_pdf::deskew::SkewDetection;

    let mut group = c.benchmark_group("deskew_detection");

    // Test detection struct creation
    group.bench_function("SkewDetection::new", |b| {
        b.iter(|| {
            black_box(SkewDetection {
                angle: 2.5,
                confidence: 0.95,
                feature_count: 100,
            })
        })
    });

    group.finish();
}

/// Benchmark margin detection structures
fn bench_margin_structures(c: &mut Criterion) {
    use superbook_pdf::margin::{ContentRect, MarginDetection, Margins};

    let mut group = c.benchmark_group("margin_structures");

    group.bench_function("Margins::uniform", |b| {
        b.iter(|| black_box(Margins::uniform(50)))
    });

    group.bench_function("Margins::struct", |b| {
        b.iter(|| {
            black_box(Margins {
                top: 10,
                bottom: 20,
                left: 30,
                right: 40,
            })
        })
    });

    group.bench_function("ContentRect::new", |b| {
        b.iter(|| {
            black_box(ContentRect {
                x: 100,
                y: 100,
                width: 1800,
                height: 2400,
            })
        })
    });

    group.bench_function("MarginDetection::new", |b| {
        b.iter(|| {
            black_box(MarginDetection {
                margins: Margins::uniform(50),
                confidence: 0.9,
                content_rect: ContentRect {
                    x: 100,
                    y: 100,
                    width: 1800,
                    height: 2400,
                },
                image_size: (2000, 3000),
            })
        })
    });

    group.finish();
}

/// Benchmark page number parsing
fn bench_page_number_parsing(c: &mut Criterion) {
    use superbook_pdf::page_number::TesseractPageDetector;

    let mut group = c.benchmark_group("page_number_parsing");

    let roman_numerals = ["i", "iv", "ix", "xiv", "xlii", "xcix"];
    for numeral in roman_numerals {
        group.bench_with_input(
            BenchmarkId::new("parse_roman", numeral),
            &numeral,
            |b, &numeral| b.iter(|| black_box(TesseractPageDetector::parse_roman_numeral(numeral))),
        );
    }

    group.finish();
}

/// Benchmark PDF metadata handling
fn bench_pdf_metadata(c: &mut Criterion) {
    use superbook_pdf::pdf_reader::PdfMetadata;

    let mut group = c.benchmark_group("pdf_metadata");

    group.bench_function("PdfMetadata::default", |b| {
        b.iter(|| black_box(PdfMetadata::default()))
    });

    group.bench_function("PdfMetadata::with_values", |b| {
        b.iter(|| {
            black_box(PdfMetadata {
                title: Some("Test Title".to_string()),
                author: Some("Test Author".to_string()),
                subject: Some("Test Subject".to_string()),
                keywords: Some("test, benchmark".to_string()),
                creator: Some("superbook-pdf".to_string()),
                producer: Some("superbook-pdf v0.1.0".to_string()),
                creation_date: None,
                modification_date: None,
            })
        })
    });

    group.finish();
}

/// Benchmark ExitCode operations
fn bench_exit_codes(c: &mut Criterion) {
    use superbook_pdf::ExitCode;

    let mut group = c.benchmark_group("exit_codes");

    group.bench_function("ExitCode::code", |b| {
        b.iter(|| black_box(ExitCode::ProcessingError.code()))
    });

    group.bench_function("ExitCode::description", |b| {
        b.iter(|| black_box(ExitCode::InputNotFound.description()))
    });

    group.bench_function("ExitCode::into_i32", |b| {
        b.iter(|| {
            let code: i32 = ExitCode::GpuError.into();
            black_box(code)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_option_builders,
    bench_presets,
    bench_utilities,
    bench_format_functions,
    bench_deskew_detection,
    bench_margin_structures,
    bench_page_number_parsing,
    bench_pdf_metadata,
    bench_exit_codes,
);

criterion_main!(benches);
