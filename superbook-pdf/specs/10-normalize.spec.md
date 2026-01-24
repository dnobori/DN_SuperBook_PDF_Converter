# 解像度正規化仕様 (normalize.rs)

## 概要

スキャン画像を統一された内部解像度 (4960x7016) に正規化する。

## アルゴリズム

### 1. アスペクト比保持リサイズ

```
入力画像 (W x H)
    │
    ▼
アスペクト比計算
    │
    ├─ W/H > target_W/target_H → 幅に合わせてリサイズ
    │
    └─ W/H ≤ target_W/target_H → 高さに合わせてリサイズ
    │
    ▼
余白を紙色で埋める
```

### 2. 紙色推定

四隅からサンプリングして背景色を推定：

```
┌─────────────────┐
│ TL          TR  │
│                 │
│                 │
│ BL          BR  │
└─────────────────┘

paper_color = average(TL, TR, BL, BR)
```

サンプリングパッチサイズ: 16x16 pixels

### 3. グラデーション背景塗りつぶし

水平方向のグラデーションで自然な背景を生成：

```
左端色 ──────────────────── 右端色
  │                           │
  ▼                           ▼
top_left_color ←──────→ top_right_color
      ↓                       ↓
      │    (線形補間)         │
      ↓                       ↓
bot_left_color ←──────→ bot_right_color
```

## パラメータ

| パラメータ | デフォルト値 | 説明 |
|-----------|-------------|------|
| target_width | 4960 | 目標幅 (pixels) |
| target_height | 7016 | 目標高さ (pixels) |
| resampler | Lanczos3 | リサンプリングアルゴリズム |
| padding_mode | GradientFill | 余白塗りつぶしモード |

## リサンプラーオプション

- `Nearest`: 最近傍補間 (高速、低品質)
- `Bilinear`: 双線形補間
- `Bicubic`: 双三次補間
- `Lanczos3`: Lanczos補間 (推奨、高品質)

## パディングモード

- `SolidColor`: 単色塗りつぶし
- `GradientFill`: グラデーション塗りつぶし (推奨)
- `Mirror`: ミラー反射

## API

```rust
// オプション構築
let options = NormalizeOptions::builder()
    .target_width(4960)
    .target_height(7016)
    .resampler(Resampler::Lanczos3)
    .padding_mode(PaddingMode::GradientFill)
    .build();

// 正規化実行
let result = ImageNormalizer::normalize(
    &input_path,
    &output_path,
    &options
)?;

// 結果
println!("Original: {}x{}", result.original_width, result.original_height);
println!("Normalized: {}x{}", result.normalized_width, result.normalized_height);
println!("Paper color: RGB({},{},{})",
    result.paper_color.r, result.paper_color.g, result.paper_color.b);
```

## テストケース

| TC ID | 説明 | 期待結果 |
|-------|------|---------|
| TC-NORM-001 | 小さい画像の正規化 | 紙色パディング追加 |
| TC-NORM-002 | 大きい画像の正規化 | リサイズ後パディング |
| TC-NORM-003 | アスペクト比異なる画像 | 比率保持 |
| TC-NORM-004 | 暗い背景画像 | 正しい紙色推定 |
| TC-NORM-005 | 白背景画像 | 白でパディング |
