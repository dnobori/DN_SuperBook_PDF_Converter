# 色統計・カラー補正仕様 (color_stats.rs)

## 概要

全ページの色統計を分析し、グローバルカラー補正パラメータを計算・適用する。

## アルゴリズム

### 1. 色統計収集

各ページから紙色(paper)とインク色(ink)を抽出：

```
輝度ヒストグラム [0-255]
    │
    ▼
┌───────────────────────────────────────┐
│     ▲                          ▲     │
│     │ ink peak           paper peak  │
│  ▒▒▒█▒▒▒                    ▒▒█▒▒    │
└───────────────────────────────────────┘
  0                                  255

paper_rgb = 上位5%の平均色
ink_rgb = 下位5%の平均色
```

### 2. 外れ値除去 (MAD法)

```
Median Absolute Deviation (MAD):
    median = median(values)
    mad = median(|values - median|)
    threshold = 2.5 * mad

    inliers = values where |v - median| < threshold
```

### 3. グローバル補正パラメータ計算

```
target_paper = (255, 255, 255)  # 白
target_ink = (0, 0, 0)          # 黒

scale_r = (target_paper_r - target_ink_r) / (median_paper_r - median_ink_r)
offset_r = target_ink_r - scale_r * median_ink_r

# 同様に G, B チャンネル
```

### 4. ゴースト抑制

薄いインク残りを除去：

```
if luminance > ghost_threshold:
    blend_to_white(pixel, t)

where t = (lum - threshold) / (255 - threshold)
```

## パラメータ

| パラメータ | デフォルト値 | 説明 |
|-----------|-------------|------|
| ghost_suppress_threshold | 245 | ゴースト抑制閾値 |
| white_clip_range | 5 | 白クリップ範囲 |
| saturation_threshold | 30 | 彩度閾値 |
| sample_step | 4 | サンプリングステップ |

## API

```rust
// ページ単位の色統計
let stats = ColorAnalyzer::calculate_stats(&image_path)?;
println!("Paper: RGB({:.1},{:.1},{:.1})",
    stats.paper_r, stats.paper_g, stats.paper_b);
println!("Ink: RGB({:.1},{:.1},{:.1})",
    stats.ink_r, stats.ink_g, stats.ink_b);

// グローバル補正パラメータ計算
let all_stats: Vec<ColorStats> = pages.iter()
    .filter_map(|p| ColorAnalyzer::calculate_stats(p).ok())
    .collect();
let global_param = ColorAnalyzer::decide_global_adjustment(&all_stats);

// 補正適用
let mut image = image::open(&path)?.to_rgb8();
ColorAnalyzer::apply_adjustment(&mut image, &global_param);
image.save(&output_path)?;
```

## テストケース

| TC ID | 説明 | 期待結果 |
|-------|------|---------|
| TC-COLOR-001 | 白背景・黒文字 | paper≈255, ink≈0 |
| TC-COLOR-002 | 黄ばんだ紙 | paper<255, 補正で白化 |
| TC-COLOR-003 | 薄いインク | ゴースト抑制で除去 |
| TC-COLOR-004 | カラー画像 | 彩度保持 |
| TC-COLOR-005 | 外れ値ページ | MADで除外 |
