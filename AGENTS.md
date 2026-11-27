# R Auto Shutdown

## 修改位置

- `src/app.rs` 和 `src/notification.rs` 分別管理主視窗與倒數通知。共用時間規則在 `src/lib.rs`，平台排程及關機執行在 `src/schedule.rs` 與 `src/shutdown.rs`。
- 編輯通知或倒數時，核對延後、取消、跨午夜及至少 30 秒的操作時間。Windows 每週排程沿用既有任務名稱和保存格式。
- 本機驗證使用模擬模式或測試替身。執行前確認操作的是測試倒數或測試排程。

## 驗證

- 最低 Rust 1.88。跑 `cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked` 和 `cargo build --release --locked`。
- 影響平台行為的改動要回報實際驗證的系統與模式。CI 建置 Windows、Linux 與 macOS，桌面操作須另行驗證。[執行與限制](README.md)
