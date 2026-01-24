# 21-websocket.spec.md - WebSocketリアルタイム更新仕様

## 概要

REST APIのポーリングに代わり、WebSocketでジョブ進捗をリアルタイムにプッシュ。

## 目的

- クライアント側のポーリング負荷削減
- 即時の進捗更新
- サーバープッシュによる効率的な通信

## 設計

### アーキテクチャ

```
┌─────────────────────────────────────────────────────┐
│                     Web Server                       │
├─────────────────────────────────────────────────────┤
│  WebSocket:                                         │
│    WS /ws/jobs/:id    - ジョブ進捗ストリーム        │
│    WS /ws/jobs        - 全ジョブ更新ストリーム      │
├─────────────────────────────────────────────────────┤
│  Message Types:                                     │
│    - progress         - 進捗更新                    │
│    - status_change    - ステータス変更              │
│    - completed        - 完了通知                    │
│    - error            - エラー通知                  │
└─────────────────────────────────────────────────────┘
```

### WebSocketメッセージ形式

#### 進捗更新
```json
{
  "type": "progress",
  "job_id": "uuid",
  "data": {
    "current_step": 5,
    "total_steps": 12,
    "step_name": "AI Upscaling",
    "percent": 42
  }
}
```

#### ステータス変更
```json
{
  "type": "status_change",
  "job_id": "uuid",
  "data": {
    "old_status": "queued",
    "new_status": "processing"
  }
}
```

#### 完了通知
```json
{
  "type": "completed",
  "job_id": "uuid",
  "data": {
    "output_path": "/api/jobs/uuid/download",
    "elapsed_seconds": 45.2,
    "page_count": 12
  }
}
```

#### エラー通知
```json
{
  "type": "error",
  "job_id": "uuid",
  "data": {
    "message": "Pipeline error: PDF extraction failed"
  }
}
```

### データ構造

```rust
#[derive(Debug, Clone, Serialize)]
pub enum WsMessage {
    Progress {
        job_id: Uuid,
        current_step: u32,
        total_steps: u32,
        step_name: String,
        percent: u8,
    },
    StatusChange {
        job_id: Uuid,
        old_status: JobStatus,
        new_status: JobStatus,
    },
    Completed {
        job_id: Uuid,
        output_path: String,
        elapsed_seconds: f64,
        page_count: usize,
    },
    Error {
        job_id: Uuid,
        message: String,
    },
}

pub struct WsBroadcaster {
    clients: RwLock<HashMap<Uuid, Vec<mpsc::Sender<WsMessage>>>>,
}
```

### WebUI更新

```javascript
// WebSocket接続
const ws = new WebSocket(`ws://${location.host}/ws/jobs/${jobId}`);

ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    switch (msg.type) {
        case 'progress':
            updateProgressBar(msg.data.percent, msg.data.step_name);
            break;
        case 'completed':
            showDownloadButton(msg.data.output_path);
            break;
        case 'error':
            showError(msg.data.message);
            break;
    }
};
```

## API

| 関数/構造体 | 説明 |
|-------------|------|
| `WsBroadcaster::new()` | ブロードキャスター生成 |
| `WsBroadcaster::subscribe()` | ジョブ購読 |
| `WsBroadcaster::broadcast()` | メッセージ配信 |
| `ws_handler()` | WebSocketハンドラ |

## テストケース

| TC ID | テスト内容 |
|-------|------------|
| WS-001 | WebSocket接続確立 |
| WS-002 | 進捗メッセージ受信 |
| WS-003 | ステータス変更通知 |
| WS-004 | 完了通知 |
| WS-005 | エラー通知 |
| WS-006 | 接続切断時のクリーンアップ |
| WS-007 | 複数クライアント同時接続 |
| WS-008 | 無効なジョブID拒否 |

## 実装ステータス

| 機能 | 状態 | 備考 |
|------|------|------|
| WsBroadcaster | 🟢 | 完了 (14テスト) |
| WebSocketハンドラ | 🟢 | 完了 (ws_job_handler) |
| WebUI WebSocket統合 | 🟢 | 完了 (ポーリングからWS移行、フォールバック付き) |
| 統合テスト | 🟢 | 完了 (14テストケース) |

## 依存クレート

```toml
[dependencies]
# axumのws機能を使用 (tokio-tungsteniteを内部で利用)
axum = { version = "0.8", features = ["multipart", "ws"] }
```

## 注意事項

- 接続タイムアウト: 5分
- ハートビート間隔: 30秒
- 最大同時接続数: 100
- REST APIは引き続き動作（後方互換性）
