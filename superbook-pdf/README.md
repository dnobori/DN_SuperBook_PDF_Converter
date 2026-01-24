# superbook-pdf

スキャンされた書籍PDFを高品質なデジタル書籍に変換するRustツール。

[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-1064_passed-green.svg)]()

## ステータス

**C#→Rust完全移行完了** - 本番環境で使用可能

| 項目 | 状態 |
|------|------|
| コード行数 | 25,900行 |
| テスト | 1,094件 全てパス |
| Clippy警告 | 0件 |
| メモリ使用量 | 0.4-0.8 GB (C#版の1/30) |

## 特徴

- **高品質画像抽出**: ImageMagick/Poppler連携による300-600 DPI画像抽出
- **傾き補正**: 自動スキュー検出と補正
- **内部解像度正規化**: 4960x7016の標準解像度に正規化
- **グローバルカラー補正**: 書籍全体の色味を統一
- **マージン最適化**: 四分位数ベース（Tukey fence）の外れ値除去
- **AI超解像**: RealESRGAN 2x/4xアップスケーリング
- **日本語OCR**: YomiToku AI-OCRによる検索可能PDF生成
- **ページ番号認識**: ローマ数字対応、オフセット自動補正
- **縦書き検出**: 日本語書籍の縦書き/横書き自動判定
- **メモリ効率**: ストリーミング処理で0.4-0.8GB RAM使用

## クイックスタート

```bash
# ビルド
cd superbook-pdf
cargo build --release

# システム情報確認
./target/release/superbook-pdf info

# 基本的な変換
./target/release/superbook-pdf convert input.pdf output/

# 高度な処理（すべての機能を有効化）
./target/release/superbook-pdf convert input.pdf output/ --advanced
```

## インストール

### ビルド要件

- Rust 1.75以上
- 以下のいずれか:
  - ImageMagick 7.x (推奨)
  - Poppler-utils (pdftoppm) - ImageMagickなしでも動作可能
- Ghostscript
- Python 3.10以上 (AI機能用、オプション)

```bash
# Ubuntu/Debian
sudo apt install poppler-utils ghostscript

# ビルド
cargo build --release
```

### Python AI環境セットアップ (オプション)

```bash
cd ai_bridge
python -m venv venv
source venv/bin/activate
pip install -r requirements.txt
```

## 使い方

### 基本コマンド

```bash
# 単一PDF変換
superbook-pdf convert input.pdf output/

# ディレクトリ一括変換
superbook-pdf convert input_dir/ output/

# プレビュー (ドライラン)
superbook-pdf convert input.pdf output/ --dry-run
```

### 処理オプション

```bash
superbook-pdf convert input.pdf output/ \
    --dpi 300 \              # 出力DPI (default: 300)
    --margin-trim 0.5 \      # マージントリム % (default: 0.5)
    --no-deskew \            # 傾き補正無効
    --no-upscale \           # AI超解像無効
    --no-gpu \               # CPU処理のみ
    -o \                     # OCR有効 (YomiToku)
    -v                       # 詳細出力
```

### 高度な処理オプション

```bash
superbook-pdf convert input.pdf output/ \
    --advanced \              # 全ての高度機能を有効化
    --internal-resolution \   # 内部解像度正規化 (4960x7016)
    --color-correction \      # グローバルカラー補正
    --offset-alignment \      # ページ番号オフセット補正
    --output-height 3508      # 出力高さ (default: 3508)
```

## 処理パイプライン

```
PDF入力
   ↓
1. 画像抽出 (pdftoppm/ImageMagick)
   ↓
2. マージントリミング (0.5%)
   ↓
3. AI超解像 (RealESRGAN 2x) [オプション]
   ↓
4. 内部解像度正規化 (4960x7016) [--advanced]
   ↓
5. 傾き補正 (Deskew)
   ↓
6. 色統計・グローバルカラー補正 [--advanced]
   ↓
7. テキストBBox検出・四分位数クロップ [--advanced]
   ↓
8. ページ番号検出・オフセット計算 [--advanced]
   ↓
9. 最終出力リサイズ (3508高さ)
   ↓
10. PDF生成 (printpdf)
```

## モジュール構成

| モジュール | 機能 | 行数 |
|-----------|------|------|
| `cli` | CLIインターフェース (clap) | ~1,600 |
| `pdf_reader` | PDF読み込み (lopdf) | ~1,600 |
| `pdf_writer` | PDF書き込み (printpdf) | ~1,800 |
| `image_extract` | 画像抽出 (pdftoppm/ImageMagick) | ~2,200 |
| `deskew/` | 傾き補正 (imageproc) | ~2,200 |
| `margin/` | マージン検出 (Tukey fence) | ~2,900 |
| `page_number/` | ページ番号認識 | ~2,600 |
| `normalize` | 内部解像度正規化 | ~500 |
| `color_stats` | カラー統計・補正 | ~600 |
| `finalize` | 最終出力処理 | ~500 |
| `ai_bridge` | Python AI連携 | ~1,000 |
| `realesrgan` | AI超解像 | ~2,000 |
| `yomitoku` | 日本語OCR | ~1,800 |

## パフォーマンス

| 項目 | C#版 | Rust版 | 改善率 |
|------|------|--------|--------|
| メモリ使用量 | 10-30 GB | 0.4-0.8 GB | **30-40倍削減** |
| OOMリスク | 頻発 | なし | **解消** |
| 依存関係 | ImageMagick必須 | pdftoppmでも動作 | **柔軟性向上** |
| 配布 | コンテナ必須 | シングルバイナリ | **簡素化** |

## 開発

```bash
# テスト実行
cargo test

# 特定モジュールのテスト
cargo test margin::

# ベンチマーク
cargo bench

# コード品質チェック
cargo clippy -- -D warnings
cargo fmt -- --check
```

## ロードマップ

### 完了 (v0.1.0)

- [x] C#版の全機能をRustに移植
- [x] 四分位数ベースマージン検出（Tukey fence）
- [x] グローバルカラー補正
- [x] ページ番号オフセット計算
- [x] PDF画像埋め込み
- [x] Poppler fallback (ImageMagickなしでも動作)

### 進行中 (v0.2.0)

- [x] 縦書き検出（日本語書籍向け） - `vertical_detect` モジュール
- [x] JPEG圧縮オプション（ファイルサイズ削減） - `--jpeg-quality` CLI引数
- [x] 並列処理の最適化 - Step 6/7/8/10を`rayon::par_iter`で並列化
- [ ] Webインターフェース

## ライセンス

AGPL-3.0

Original Author: 登 大遊 (Daiyuu Nobori)

## 関連リンク

- [オリジナルリポジトリ](https://github.com/dnobori/DN_SuperBook_PDF_Converter)
- [開発ロードマップ (Issue #19)](https://github.com/clearclown/DN_SuperBook_PDF_Converter_Linux/issues/19)
- [RealESRGAN](https://github.com/xinntao/Real-ESRGAN)
- [YomiToku](https://github.com/kotaro-kinoshita/yomitoku)
