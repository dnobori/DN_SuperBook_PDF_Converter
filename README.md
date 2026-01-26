# superbook-pdf

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

> **Fork of [dnobori/DN_SuperBook_PDF_Converter](https://github.com/dnobori/DN_SuperBook_PDF_Converter)**
>[フォーク元の素晴らしき芸術的なREADME.md](https://github.com/dnobori/DN_SuperBook_PDF_Converter/blob/master/README.md) : 正直、これを読めばすべてがわかる
> Rust で完全リライトしたスキャン書籍 PDF 高品質化ツール

**オリジナル著者:** 登 大遊 (Daiyuu Nobori) 様
**Rust リライト:** clearclown
**ライセンス:** AGPL v3.0

---

## Before / After

![Before and After comparison](doc_img/ba.png)

| | Before (左) | After (右) |
|---|---|---|
| **解像度** | 1242×2048 px | 2363×3508 px |
| **ファイルサイズ** | 981 KB | 1.6 MB |
| **品質** | ぼやけ、低コントラスト | 鮮明、高コントラスト |

> RealESRGAN による AI 超解像で、文字のエッジが鮮明になり、読みやすさが大幅に向上

---

## 特徴

- **Rust 実装** - C# 版を完全リライト。メモリ効率大幅改善
- **AI 超解像** - RealESRGAN で画像を高解像度化
- **日本語 OCR** - YomiToku による文字認識
- **傾き補正** - 大津二値化 + Hough 変換で自動補正
- **Web UI** - ブラウザから操作可能

---

## 必要なもの

### 必須

| 項目 | 要件 |
|------|------|
| OS | Linux (Ubuntu 20.04+, Debian 11+) |
| Rust | 1.75 以上 |
| Poppler | `pdftoppm` コマンド |

### AI機能を使う場合 (オプション)

| 項目 | 要件 |
|------|------|
| Python | 3.10 以上 |
| GPU | NVIDIA GPU (CUDA 11.8+, VRAM 4GB以上推奨) |

---

## インストール

### 1. システム依存パッケージ

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y poppler-utils python3 python3-venv
```

```sh
# Fedora
sudo dnf install -y poppler-utils python3
```

### 2. Rust ツールのインストール

**crates.io からインストール (推奨):**

```bash
cargo install superbook-pdf --features web
```

**ソースからビルド:**

```bash
git clone https://github.com/clearclown/DN_SuperBook_PDF_Converter_Linux.git
cd DN_SuperBook_PDF_Converter_Linux/superbook-pdf
cargo build --release --features web
```

バイナリは `target/release/superbook-pdf` に生成されます。

### 3. AI機能のセットアップ (オプション)

AI超解像 (RealESRGAN) と OCR (YomiToku) を使用する場合:

```bash
cd ai_bridge

# Python 仮想環境を作成
python3 -m venv .venv
source .venv/bin/activate

# GPU版 PyTorch をインストール (NVIDIA GPU使用時)
pip install torch torchvision --index-url https://download.pytorch.org/whl/cu121

# AI依存パッケージをインストール
pip install -r requirements.txt
```

**実行時に環境変数を設定:**

```bash
export SUPERBOOK_VENV=/path/to/superbook-pdf/ai_bridge/.venv
```

### 4. Docker/Podman で実行 (オプション)

GPU対応コンテナで簡単にセットアップできます:

```bash
cd superbook-pdf

# Docker
docker compose up -d

# Podman (4.7+)
podman compose up -d

# ブラウザで http://localhost:8080 を開く
```

**単体コンテナで実行:**

```bash
# ビルド
podman build -t superbook-pdf .

# 実行 (GPU使用)
podman run --rm -it --gpus all \
  -v ./input:/data/input:ro \
  -v ./output:/data/output:rw \
  superbook-pdf convert /data/input/book.pdf -o /data/output/
```

---

## 使い方

### 基本的な変換

```bash
# シンプルな変換 (傾き補正 + AI超解像)
./target/release/superbook-pdf convert input.pdf -o output/

# 高品質変換 (全オプション有効 + OCR)
./target/release/superbook-pdf convert input.pdf -o output/ --advanced --ocr

# GPU無効化 (CPUのみで処理)
./target/release/superbook-pdf convert input.pdf -o output/ --no-gpu
```

### オプション一覧

```
superbook-pdf convert [OPTIONS] <INPUT>

引数:
  <INPUT>              入力PDFファイル

オプション:
  -o, --output <DIR>   出力ディレクトリ [デフォルト: ./output]
  --advanced           高品質処理を有効化
  --ocr                OCR を有効化 (YomiToku)
  --no-gpu             GPU を無効化
  --no-upscale         AI超解像を無効化
  --no-deskew          傾き補正を無効化
  --max-pages <N>      処理ページ数を制限 (テスト用)
  -v, --verbose        詳細出力 (-vvv で最大)
  -h, --help           ヘルプ表示
```

### Web UI

```bash
./target/release/superbook-pdf serve --port 8080
# ブラウザで http://localhost:8080 を開く
```

---

## 処理パイプライン

1. **PDF画像抽出** - pdftoppm で 300 DPI 抽出
2. **マージントリム** - 0.5% の余白を除去
3. **AI超解像** - RealESRGAN で 2x アップスケール
4. **傾き補正** - 大津二値化 + Hough変換
5. **カラー補正** - 紙色の白化 (--advanced)
6. **PDF生成** - メタデータ同期
7. **OCR** - YomiToku (--ocr)

---

## トラブルシューティング

| 問題 | 解決策 |
|------|--------|
| `pdftoppm: command not found` | `sudo apt install poppler-utils` |
| RealESRGAN が動かない | `SUPERBOOK_VENV` 環境変数を設定 |
| GPU が使用されない | `pip install torch --index-url https://download.pytorch.org/whl/cu121` |
| メモリ不足 | `--max-pages 10` で分割処理 |

---

## 開発

```bash
# テスト実行
cargo test --features web

# フォーマット
cargo fmt && cargo clippy
```

---

## ライセンス

AGPL v3.0 - [LICENSE](LICENSE)

---

## 謝辞

- **登 大遊 (Daiyuu Nobori)** - オリジナル実装
- **[RealESRGAN](https://github.com/xinntao/Real-ESRGAN)** - AI超解像
- **[YomiToku](https://github.com/kotaro-kinoshita/yomitoku)** - 日本語OCR
