# 最終出力処理仕様 (finalize.rs)

## 概要

1. 目標高さへのリサイズ
2. クロップ領域適用
3. シフトオフセット適用
4. 紙色保持パディング（Phase 5追加）

## アルゴリズム

### 1. リサイズ処理

```
入力画像 (W x H)
    │
    ▼
目標高さ計算
    target_height = 3508 (default)
    scale = target_height / H
    target_width = W * scale
    │
    ▼
Lanczos3リサンプリング
    │
    ▼
出力画像 (target_width x target_height)
```

### 2. クロップ領域適用

```
┌─────────────────────────────────────┐
│                                     │
│    ┌─────────────────────┐          │
│    │   crop_region       │          │
│    │                     │          │
│    └─────────────────────┘          │
│                                     │
└─────────────────────────────────────┘

output = input.crop(
    crop_region.left,
    crop_region.top,
    crop_region.width,
    crop_region.height
)
```

### 3. シフトオフセット適用

```
元の位置          シフト後
┌─────────┐      ┌─────────┐
│  ┌───┐  │      │    ┌───┐│
│  │   │  │  →   │    │   ││
│  └───┘  │      │    └───┘│
└─────────┘      └─────────┘
           shift_x, shift_y
```

### 4. 紙色保持パディング

シフトで生じた余白を紙色で塗りつぶし：

```
paper_color = estimate_from_corners()
fill_margin(shifted_image, paper_color)
```

## パラメータ

| パラメータ | デフォルト値 | 説明 |
|-----------|-------------|------|
| target_height | 3508 | 目標高さ (pixels) |
| margin_percent | 0 | 追加マージン% |
| feather_pixels | 0 | エッジフェザリング |
| resampler | Lanczos3 | リサンプリング |

## API

```rust
// オプション構築
let options = FinalizeOptions::builder()
    .target_height(3508)
    .margin_percent(0)
    .build();

// 単一ページ処理
let result = PageFinalizer::finalize(
    &input_path,
    &output_path,
    &options,
    Some(crop_region),  // Option<CropRegion>
    shift_x,            // i32
    shift_y,            // i32
)?;

println!("Input: {}x{}", result.input_width, result.input_height);
println!("Output: {}x{}", result.output_width, result.output_height);

// バッチ処理
let results = PageFinalizer::finalize_batch(
    &pages,             // Vec<(PathBuf, PathBuf, bool)>
    &options,
    Some(odd_crop),
    Some(even_crop),
    &page_shifts,       // Vec<(i32, i32)>
)?;
```

## テストケース

| TC ID | 説明 | 期待結果 |
|-------|------|---------|
| TC-FINAL-001 | 標準リサイズ | 3508高さ |
| TC-FINAL-002 | クロップ適用 | 正確なクロップ |
| TC-FINAL-003 | シフト適用 | 位置移動 |
| TC-FINAL-004 | 紙色パディング | 自然な余白 |
| TC-FINAL-005 | バッチ処理 | 全ページ処理 |
