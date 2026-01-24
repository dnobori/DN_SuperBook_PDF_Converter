# 並列処理モジュール仕様書

## 概要

Rayonを使用した画像処理パイプラインの並列化。
CPU コア数を活用して処理速度を向上させる。

## 実装状況: ✅ 完了

| 機能 | 状態 | 備考 |
|------|------|------|
| parallel.rs モジュール | ✅ | ParallelProcessor, ParallelResult |
| Step 6: 正規化 | ✅ | par_iter + zip |
| Step 7: 色統計収集 | ✅ | par_iter + enumerate |
| Step 7: 色補正適用 | ✅ | par_iter + zip |
| Step 8a: BBox検出 | ✅ | GroupCropAnalyzer内で並列化 |
| Step 8c: クロップ適用 | ✅ | par_iter + zip |
| Step 10: 最終出力 | ✅ | par_iter + zip |

## TC ID一覧

| TC ID | テスト項目 | 優先度 | 状態 |
|-------|-----------|--------|------|
| PAR-001 | 並列画像抽出 | 高 | ✅ |
| PAR-002 | 並列Deskew処理 | 高 | - |
| PAR-003 | 並列正規化処理 | 高 | ✅ |
| PAR-004 | 並列色補正 | 中 | ✅ |
| PAR-005 | 並列マージン検出 | 中 | ✅ |
| PAR-006 | スレッド数制限 | 高 | ✅ |
| PAR-007 | エラーハンドリング | 高 | ✅ |
| PAR-008 | 進捗報告 | 中 | ✅ |
| PAR-009 | メモリ使用量制御 | 高 | ✅ |
| PAR-010 | キャンセル対応 | 低 | - |

## 並列化対象

### 1. 独立したページ処理 (並列化可能)

```
各ページに対して独立して実行できる処理:
- 画像抽出 (PDFからの抽出は順次、後処理は並列)
- Deskew (傾き補正)
- 内部解像度正規化
- RealESRGAN アップスケール (GPU使用時は注意)
- 最終出力リサイズ
```

### 2. 全ページ依存処理 (順次実行)

```
全ページの結果を集約する処理:
- グローバルカラー補正 (全ページの統計が必要)
- 四分位数ベースクロップ (全ページのBBoxが必要)
- ページ番号オフセット計算 (全ページの番号が必要)
- 縦書き検出 (複数ページのサンプリングが必要)
```

## 公開API

```rust
/// 並列処理オプション
#[derive(Debug, Clone)]
pub struct ParallelOptions {
    /// スレッド数 (0 = 自動検出)
    pub num_threads: usize,
    /// チャンクサイズ (0 = 自動)
    pub chunk_size: usize,
    /// メモリ制限 (MB, 0 = 無制限)
    pub memory_limit_mb: usize,
}

/// 並列処理結果
pub struct ParallelResult<T> {
    pub results: Vec<T>,
    pub errors: Vec<(usize, String)>,
    pub duration: Duration,
}

/// 並列画像処理
pub fn parallel_process<T, F>(
    inputs: &[PathBuf],
    processor: F,
    options: &ParallelOptions,
) -> ParallelResult<T>
where
    F: Fn(&Path) -> Result<T, Error> + Sync + Send,
    T: Send;
```

## 実装方針

### Rayonの使用

```rust
use rayon::prelude::*;

// 基本的な並列処理
let results: Vec<_> = images
    .par_iter()
    .map(|img_path| process_image(img_path))
    .collect();

// エラーハンドリング付き
let results: Vec<Result<_, _>> = images
    .par_iter()
    .map(|img_path| process_image(img_path))
    .collect();

let (successes, errors): (Vec<_>, Vec<_>) = results
    .into_iter()
    .partition(Result::is_ok);
```

### スレッドプール設定

```rust
// グローバルスレッドプール設定
rayon::ThreadPoolBuilder::new()
    .num_threads(options.num_threads)
    .build_global()
    .unwrap();

// ローカルスレッドプール (推奨)
let pool = rayon::ThreadPoolBuilder::new()
    .num_threads(options.num_threads)
    .build()
    .unwrap();

pool.install(|| {
    images.par_iter().for_each(|img| process(img));
});
```

### メモリ制御

```rust
// チャンク処理でメモリ使用量を制御
for chunk in images.chunks(chunk_size) {
    let results: Vec<_> = chunk
        .par_iter()
        .map(|img| process(img))
        .collect();

    // チャンクごとに結果を保存
    save_chunk_results(&results)?;
}
```

## パイプライン統合

### 現在の処理フロー (順次)

```
Step 1: 画像抽出
Step 2: マージントリム
Step 3: RealESRGAN
Step 4: 内部解像度正規化
Step 5: Deskew
Step 6: 色統計収集
Step 7: グローバルカラー補正
Step 8: BBox検出・クロップ
Step 9: ページ番号検出
Step 10: 最終出力
Step 11: OCR
Step 12: PDF生成
```

### 並列化後の処理フロー

```
Step 1: 画像抽出 [順次]
Step 2-5: 並列処理ブロック1 [並列]
  - マージントリム
  - RealESRGAN
  - 内部解像度正規化
  - Deskew
Step 6: 色統計収集 [並列→集約]
Step 7: グローバルカラー補正 [並列]
Step 8a: BBox検出 [並列]
Step 8b: クロップ領域計算 [順次]
Step 8c: クロップ適用 [並列]
Step 9: ページ番号検出 [並列→集約]
Step 10: 最終出力 [並列]
Step 11: OCR [順次 - GPU制約]
Step 12: PDF生成 [順次]
```

## CLIオプション

```bash
# スレッド数指定
superbook-pdf convert input.pdf output/ --threads 8

# 自動 (CPU コア数)
superbook-pdf convert input.pdf output/ --threads 0

# チャンクサイズ指定 (メモリ節約)
superbook-pdf convert input.pdf output/ --chunk-size 10
```

## 依存関係

```toml
[dependencies]
rayon = "1.10"
num_cpus = "1.16"
```

## 参照

- [Rayon Documentation](https://docs.rs/rayon)
- C#版: 順次処理のみ（並列化なし）
