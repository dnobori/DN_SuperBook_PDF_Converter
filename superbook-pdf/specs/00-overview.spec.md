# superbook-pdf 仕様概要

## プロジェクト概要

superbook-pdf は、スキャンされた書籍PDFを高品質なデジタル書籍に変換するRust製CLIツール。

## アーキテクチャ

```
┌─────────────────────────────────────────────────────────────────┐
│                        CLI (main.rs)                            │
│  superbook-pdf convert <input> [output] [options]               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Processing Pipeline                          │
├─────────────────────────────────────────────────────────────────┤
│  1. PDF Reading      (pdf_reader.rs)                            │
│  2. Image Extraction (image_extract.rs)                         │
│  3. Deskew           (deskew.rs)                                │
│  4. Margin Trim      (margin.rs)                                │
│  5. AI Upscaling     (realesrgan.rs) ← Python Bridge            │
│  6. Normalize        (normalize.rs)        ← Phase 1            │
│  7. Color Correction (color_stats.rs)      ← Phase 2            │
│  8. Group Crop       (margin.rs)           ← Phase 3            │
│  9. Page Offset      (page_number.rs)      ← Phase 4            │
│ 10. Finalize         (finalize.rs)         ← Phase 5            │
│ 11. OCR              (yomitoku.rs) ← Python Bridge              │
│ 12. PDF Writing      (pdf_writer.rs)                            │
└─────────────────────────────────────────────────────────────────┘
```

## モジュール一覧

| モジュール | ファイル | 概要 |
|-----------|---------|------|
| CLI | cli.rs | コマンドライン引数解析 |
| PDF Reader | pdf_reader.rs | PDF読み込み、メタデータ抽出 |
| PDF Writer | pdf_writer.rs | PDF生成、OCRレイヤー埋め込み |
| Image Extract | image_extract.rs | PDF→画像抽出 (ImageMagick) |
| Deskew | deskew.rs | 傾き検出・補正 |
| Margin | margin.rs | マージン検出・トリミング・グループクロップ |
| Normalize | normalize.rs | 内部解像度正規化 (4960x7016) |
| Color Stats | color_stats.rs | 色統計・グローバルカラー補正 |
| Page Number | page_number.rs | ページ番号検出・オフセット計算 |
| Finalize | finalize.rs | 最終出力処理・リサイズ |
| RealESRGAN | realesrgan.rs | AI画像鮮明化 (Python連携) |
| YomiToku | yomitoku.rs | 日本語OCR (Python連携) |
| AI Bridge | ai_bridge.rs | Python subprocess管理 |
| Util | util.rs | ユーティリティ関数 |

## CLIオプション

### 基本オプション

| オプション | デフォルト | 説明 |
|-----------|-----------|------|
| `--dpi` | 300 | 出力DPI |
| `--ocr` | false | 日本語OCR有効化 |
| `--upscale` | true | AI Upscaling有効化 |
| `--deskew` | true | 傾き補正有効化 |
| `--margin-trim` | 0.5 | マージントリム% |
| `--gpu` | true | GPU処理有効化 |
| `--threads` | auto | 並列スレッド数 |

### 高度処理オプション (Phase 1-6)

| オプション | デフォルト | 説明 |
|-----------|-----------|------|
| `--internal-resolution` | false | 内部解像度正規化 (4960x7016) |
| `--color-correction` | false | グローバルカラー補正 |
| `--offset-alignment` | false | ページ番号オフセット補正 |
| `--output-height` | 3508 | 出力高さ (pixels) |
| `--advanced` | false | 全高度機能一括有効化 |

## 品質目標

- テスト: 1,100+ テストケース
- メモリ使用量: <3GB (C#版: 10-30GB)
- 処理速度: C#版と同等以上
- 出力品質: C#版と視覚的に同等

## 実装ステータス

| 項目 | 状態 | 備考 |
|------|------|------|
| テスト | ✅ 1,220件 | 全てパス、Clippy警告0件 |
| メモリ使用量 | ✅ 0.4-0.8GB | C#版の1/30 |
| C#→Rust移行 | ✅ 100% | 全機能移植完了 |
| main.rs リファクタリング | ✅ 完了 | 1,234行→394行 (68%削減) |
| パイプラインモジュール | ✅ 完了 | 13ステップ完全実装 |
| キャッシュ機能 | ✅ 完了 | sha256ベース |
| Poppler対応 | ✅ 完了 | ImageMagickなしでも動作 |

## 外部依存

### 必須 (どちらか1つ)

- Poppler (pdftoppm) - 推奨、軽量
- ImageMagick (PDF→画像抽出) - 代替

### ページ番号検出用

- Tesseract OCR (ページ番号検出)

### オプション (AI機能)

- Python 3.10+
- RealESRGAN (画像鮮明化)
- YomiToku (日本語OCR)

## 追加CLI機能

| オプション | 説明 |
|-----------|------|
| `--force` / `-f` | キャッシュを無視して再処理 |
| `--max-pages` | デバッグ用ページ数制限 |
| `--save-debug` | 中間画像を保存 |
| `--skip-existing` | 既存ファイルをスキップ |
| `cache-info <PDF>` | キャッシュ情報表示サブコマンド |
