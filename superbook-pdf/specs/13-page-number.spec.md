# ページ番号検出・オフセット計算仕様 (page_number.rs)

## 概要

1. Tesseract OCRによるページ番号検出
2. 物理-論理ページ番号対応計算
3. ページ番号位置ベースのオフセット計算（Phase 4追加）

## アルゴリズム

### 1. ページ番号検出

ページ下部領域をOCRスキャン：

```
┌─────────────────────────────────────┐
│                                     │
│          (本文領域)                 │
│                                     │
├─────────────────────────────────────┤
│  scan_region (下部15%)              │
│        ┌───┐                        │
│        │123│ ← ページ番号候補       │
│        └───┘                        │
└─────────────────────────────────────┘
```

検出条件:
- 数字のみで構成
- 1-9999の範囲
- ページ下部に位置

### 2. ページ番号シフト計算

```
physical_page: 0, 1, 2, 3, 4, 5, ...
detected_num:  -, -, 1, 2, 3, 4, ...

best_shift = argmax(match_count)
           = -2 (この例では)

logical_page = physical_page + shift
```

シフト探索範囲: -300 ~ +300

### 3. オフセット計算 (Phase 4)

ページ番号位置からX/Yオフセットを計算：

```
odd_pages:  平均X位置 → odd_avg_x
even_pages: 平均X位置 → even_avg_x

shift_x = target_x - detected_x
shift_y = target_y - detected_y
```

## パラメータ

| パラメータ | デフォルト値 | 説明 |
|-----------|-------------|------|
| scan_region_ratio | 0.15 | スキャン領域比率 |
| min_match_count | 5 | 最小マッチ数 |
| min_match_ratio | 0.333 | 最小マッチ比率 |
| max_shift_test | 300 | 最大シフト探索値 |

## API

```rust
// ページ番号検出
let options = PageNumberOptions::default();
let detection = TesseractPageDetector::detect_single(&image_path, page_index, &options)?;

if let Some(num) = detection.number {
    println!("Page {}: detected number {}", page_index + 1, num);
    if let Some(rect) = &detection.position {
        println!("  Position: ({}, {}) {}x{}",
            rect.x, rect.y, rect.width, rect.height);
    }
}

// バッチ検出
let detections = TesseractPageDetector::detect_batch(&image_paths, &options)?;

// オフセット分析 (Phase 4)
let analysis = PageOffsetAnalyzer::analyze_offsets(&detections, image_height);

println!("Page number shift: {}", analysis.page_number_shift);
println!("Confidence: {:.1}%", analysis.confidence * 100.0);
println!("Odd avg X: {:?}", analysis.odd_avg_x);
println!("Even avg X: {:?}", analysis.even_avg_x);

// 欠損オフセット補間
PageOffsetAnalyzer::interpolate_missing_offsets(&mut analysis, total_pages);
```

## テストケース

| TC ID | 説明 | 期待結果 |
|-------|------|---------|
| TC-PAGENUM-001 | 連続ページ番号 | 正確なシフト計算 |
| TC-PAGENUM-002 | 欠損ページ番号 | 補間で補完 |
| TC-PAGENUM-003 | 装飾的番号 | 正確な検出 |
| TC-PAGENUM-004 | ローマ数字 | 検出スキップ |
| TC-PAGENUM-005 | 奇偶位置差 | 個別オフセット |
