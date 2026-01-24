# マージン検出・グループクロップ仕様 (margin.rs)

## 概要

1. 個別ページのマージン検出
2. 統一マージン計算
3. Tukey fenceベースのグループクロップ（Phase 3追加）

## アルゴリズム

### 1. コンテンツ境界検出

```
┌─────────────────────────────────────┐
│  margin_top                         │
│  ┌─────────────────────────────┐    │
│  │                             │ m  │
│ m│     Content Area            │ a  │
│ a│                             │ r  │
│ r│                             │ g  │
│ g│                             │ i  │
│ i│                             │ n  │
│ n│                             │ _  │
│ _│                             │ r  │
│ l│                             │ i  │
│ e│                             │ g  │
│ f│                             │ h  │
│ t│                             │ t  │
│  └─────────────────────────────┘    │
│  margin_bottom                      │
└─────────────────────────────────────┘
```

検出方法:
- 行/列ごとの非白ピクセル数をカウント
- 閾値以上の行/列をコンテンツ領域とする

### 2. 統一マージン計算

全ページの最小マージンを使用：

```
unified.top = min(page.margin_top for all pages)
unified.bottom = min(page.margin_bottom for all pages)
unified.left = min(page.margin_left for all pages)
unified.right = min(page.margin_right for all pages)
```

### 3. Tukey Fence グループクロップ (Phase 3)

四分位数ベースの外れ値除去：

```
Q1 = 25th percentile
Q3 = 75th percentile
IQR = Q3 - Q1

lower_fence = Q1 - k * IQR
upper_fence = Q3 + k * IQR

where k = 1.5 (Tukey's constant)

inliers = values where lower_fence <= v <= upper_fence
```

奇数/偶数ページを個別に処理：

```
odd_pages  = [1, 3, 5, 7, ...]  → odd_crop_region
even_pages = [2, 4, 6, 8, ...]  → even_crop_region
```

## パラメータ

| パラメータ | デフォルト値 | 説明 |
|-----------|-------------|------|
| default_trim_percent | 0.5 | デフォルトトリム% |
| background_threshold | 240 | 背景色閾値 |
| min_content_ratio | 0.01 | 最小コンテンツ比率 |
| tukey_k | 1.5 | Tukey fence定数 |

## API

```rust
// 個別ページ検出
let options = MarginOptions::builder()
    .default_trim_percent(0.5)
    .build();
let detection = ImageMarginDetector::detect(&image_path, &options)?;

// 統一マージン計算
let unified = ImageMarginDetector::detect_unified(&image_paths, &options)?;

// トリミング
let result = ImageMarginDetector::trim(&input, &output, &unified.margins)?;

// グループクロップ (Phase 3)
let bounding_boxes = GroupCropAnalyzer::detect_all_bounding_boxes(&images, 240);
let unified_regions = GroupCropAnalyzer::unify_odd_even_regions(&bounding_boxes);

println!("Odd: {}x{} at ({},{})",
    unified_regions.odd_region.width,
    unified_regions.odd_region.height,
    unified_regions.odd_region.left,
    unified_regions.odd_region.top);
```

## テストケース

| TC ID | 説明 | 期待結果 |
|-------|------|---------|
| TC-MARGIN-001 | 均一マージン | 正確な検出 |
| TC-MARGIN-002 | 不均一マージン | 統一計算 |
| TC-MARGIN-003 | マージンなし | ゼロマージン |
| TC-MARGIN-004 | 外れ値ページ | Tukey除外 |
| TC-MARGIN-005 | 奇偶ページ差 | 個別リージョン |
