# 20-web.spec.md - Webインターフェース仕様

## 概要

ブラウザからPDF変換を実行できるWebインターフェース。REST APIとシンプルなWebUIを提供。

## 目的

- CLIを使わずにPDF変換を実行
- 変換進捗のリアルタイム表示
- 変換結果のダウンロード
- 複数ファイルの一括処理

## 設計

### アーキテクチャ

```
┌─────────────────────────────────────────────────────┐
│                     Web Server                       │
│                  (axum + tower)                      │
├─────────────────────────────────────────────────────┤
│  Routes:                                            │
│    POST /api/convert     - 変換ジョブ開始           │
│    GET  /api/jobs/:id    - ジョブ状態取得           │
│    GET  /api/jobs/:id/download - 結果ダウンロード   │
│    DELETE /api/jobs/:id  - ジョブキャンセル         │
│    GET  /api/health      - ヘルスチェック           │
│    GET  /                - WebUI                    │
├─────────────────────────────────────────────────────┤
│  Background Workers:                                │
│    - JobQueue (tokio::mpsc)                         │
│    - PipelineRunner (既存パイプライン再利用)        │
└─────────────────────────────────────────────────────┘
```

### REST API

#### POST /api/convert

PDFファイルをアップロードして変換ジョブを開始。

**Request:**
```
Content-Type: multipart/form-data

file: <PDF binary>
options: {
  "dpi": 300,
  "deskew": true,
  "upscale": true,
  "ocr": false,
  "advanced": false
}
```

**Response:**
```json
{
  "job_id": "uuid-v4",
  "status": "queued",
  "created_at": "2024-01-01T00:00:00Z"
}
```

#### GET /api/jobs/:id

ジョブの現在の状態を取得。

**Response:**
```json
{
  "job_id": "uuid-v4",
  "status": "processing",
  "progress": {
    "current_step": 5,
    "total_steps": 12,
    "step_name": "Deskew Correction",
    "percent": 42
  },
  "created_at": "2024-01-01T00:00:00Z",
  "started_at": "2024-01-01T00:00:10Z"
}
```

**Status values:**
- `queued` - キュー待ち
- `processing` - 処理中
- `completed` - 完了
- `failed` - 失敗
- `cancelled` - キャンセル

#### GET /api/jobs/:id/download

変換結果のPDFをダウンロード。

**Response:**
- `200 OK` + PDF binary (status: completed)
- `404 Not Found` (ジョブが存在しない)
- `409 Conflict` (status: processing/queued)

#### DELETE /api/jobs/:id

実行中のジョブをキャンセル。

**Response:**
```json
{
  "job_id": "uuid-v4",
  "status": "cancelled"
}
```

#### GET /api/health

ヘルスチェック。

**Response:**
```json
{
  "status": "healthy",
  "version": "0.5.0",
  "tools": {
    "poppler": true,
    "tesseract": false,
    "realesrgan": false,
    "yomitoku": false
  }
}
```

### WebUI

シンプルなHTML/CSS/JSによるフロントエンド。

**機能:**
- ドラッグ&ドロップでPDFアップロード
- 変換オプション設定
- 進捗バー表示
- 結果ダウンロードボタン

**技術:**
- 静的ファイル埋め込み (rust-embed)
- WebSocket for リアルタイム進捗 (v0.5.0で実装完了)

### データ構造

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub status: JobStatus,
    pub options: ConvertOptions,
    pub progress: Option<Progress>,
    pub input_filename: String,
    pub output_path: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub current_step: u32,
    pub total_steps: u32,
    pub step_name: String,
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertOptions {
    pub dpi: u32,
    pub deskew: bool,
    pub upscale: bool,
    pub ocr: bool,
    pub advanced: bool,
}
```

### CLIサブコマンド

```bash
# Webサーバー起動
superbook-pdf serve [OPTIONS]

Options:
  -p, --port <PORT>     ポート番号 [default: 8080]
  -b, --bind <ADDR>     バインドアドレス [default: 127.0.0.1]
  --workers <N>         ワーカースレッド数 [default: CPUs]
  --upload-limit <MB>   アップロード上限 [default: 500]
  --job-timeout <SEC>   ジョブタイムアウト [default: 3600]
```

## API

| 関数/構造体 | 説明 |
|-------------|------|
| `WebServer::new()` | サーバーインスタンス生成 |
| `WebServer::run()` | サーバー起動 (async) |
| `JobQueue::submit()` | ジョブ投入 |
| `JobQueue::get()` | ジョブ取得 |
| `JobQueue::cancel()` | ジョブキャンセル |

## テストケース

| TC ID | テスト内容 |
|-------|------------|
| WEB-001 | サーバー起動・停止 |
| WEB-002 | ヘルスチェックエンドポイント |
| WEB-003 | PDFアップロード |
| WEB-004 | ジョブ状態取得 |
| WEB-005 | 結果ダウンロード |
| WEB-006 | ジョブキャンセル |
| WEB-007 | 不正なリクエスト (400) |
| WEB-008 | 存在しないジョブ (404) |
| WEB-009 | 並行ジョブ処理 |
| WEB-010 | アップロードサイズ制限 |
| WEB-011 | タイムアウト処理 |
| WEB-012 | WebUI静的ファイル配信 |

## 実装ステータス

| 機能 | 状態 | 備考 |
|------|------|------|
| サーバー基盤 | 🟢 | 完了 (axum + tower) |
| REST API | 🟢 | 完了 (5エンドポイント) |
| ジョブキュー | 🟢 | 完了 (25テスト) |
| CLIコマンド | 🟢 | 完了 (serve サブコマンド) |
| バックグラウンド処理 | 🟢 | 完了 (WorkerPool + JobWorker) |
| パイプライン統合 | 🟢 | 完了 (PdfPipeline + WebProgressCallback) |
| WebUI | 🟢 | 完了 (rust-embed + ドラッグ&ドロップ) |
| 統合テスト | 🟢 | 完了 (14テストケース) |

## 依存クレート

```toml
[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["fs", "cors", "limit"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
rust-embed = "8"
```

## 注意事項

- 単一ユーザー向け（認証なし）
- ローカルホストデフォルト（セキュリティ考慮）
- 同時処理数はワーカー数で制限
- 一時ファイルは自動クリーンアップ
