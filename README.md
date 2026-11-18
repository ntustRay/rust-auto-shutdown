# Rust Auto Shutdown

以 Rust 製作的 Windows 自動關機工具，提供繁體中文視窗、每週重複排程與單次倒數。首次使用預設為模擬模式；啟用真正關機前需要再次確認。

A small Windows desktop shutdown timer with weekly schedules and simulation mode.

## 功能

| 模式 | 設定方式 | 關閉視窗後 |
| --- | --- | --- |
| 每週排程 | 多選星期與 `HH:MM`，可快速選取每天或平日 | 仍由 Windows 工作排程器執行 |
| 單次倒數 | 1–10,080 分鐘，或下一個本機 `HH:MM` | 取消本次倒數 |

- 每週預設為每天 `23:30`、模擬模式，按下「儲存並啟用排程」後才生效。
- 模擬排程會實際註冊 Windows 任務，到期只保存執行紀錄，不呼叫關機。
- 編輯中的預覽與已生效設定分別顯示；變更星期、時間或模式後，需重新儲存。
- 真實模式在開始單次倒數或儲存每週排程前，會顯示確認畫面。
- 單次指定時刻已過或等於目前分鐘時，排到明天。

## 執行

需求：Windows 10／11、Rust 1.88 以上，以及 Rust Windows 工具鏈需要的 C++ 建置工具。專案使用 Rust 2024 edition。

```powershell
cargo run --release
```

建置後可直接執行 `target\release\rust-auto-shutdown.exe`。

### 每週排程

1. 在「每週排程」選擇至少一個星期，輸入 24 小時制時間，例如 `23:30`。
2. 先保留模擬模式，按下「儲存並啟用排程」。確認畫面顯示已生效設定。
3. 到期後查看最近執行紀錄；模擬模式不會關機。
4. 需要真正關機時，解除模擬模式，重新儲存並完成確認。
5. 要停止後續執行，按下「停用每週排程」。關閉視窗不會停用每週排程。

重新開啟程式會載入已儲存的每週設定，包括其模擬／真實模式。

### 單次倒數

切換至「單次倒數」，設定分鐘數或指定時刻後啟動。到期前可取消，關閉視窗也會取消。每週排程與單次倒數互相獨立；取消其中一種不會停用另一種。

### 命令列參數

| 參數 | 用途 |
| --- | --- |
| 無參數 | 開啟視窗，載入已儲存的每週設定 |
| `--dry-run` | 相容啟動旗標；開啟同一視窗，不覆寫已儲存設定或停用既有任務 |
| `--help`、`-h` | 顯示說明並結束 |
| `--version`、`-V` | 顯示版本並結束 |

排程由視窗設定，未知參數或不支援的參數組合以結束碼 `2` 結束。內部 `--scheduled-dry-run`／`--scheduled-shutdown` 入口供工作排程器使用，不是命令列設定排程功能。

## 本機保存與停用

每週排程以目前 Windows 使用者的身分執行，不要求或保存 Windows 密碼，也不安裝背景服務。任務使用 `InteractiveToken` 與 `LeastPrivilege`；一般使用者可建立自身任務，但系統政策或權限仍可能拒絕建立。[Microsoft 任務安全性文件](https://learn.microsoft.com/en-us/windows/win32/taskschd/security-contexts-for-running-tasks)

| 項目 | 位置／內容 |
| --- | --- |
| 專用 Windows 任務 | `RustAutoShutdown-<目前使用者 SID>` |
| 執行檔副本 | `%LOCALAPPDATA%\RustAutoShutdown\app.exe` |
| 每週設定 | 同目錄 `schedule.json`：星期一至日、時間、模擬模式 |
| 最近執行紀錄 | 同目錄 `last-run.json`：預定時刻、完成時間、模式、結果及訊息 |

儲存時先註冊任務，再提交設定；設定提交失敗時會嘗試復原原有任務。若復原也失敗，程式會顯示需要處理的任務名稱。

停用只刪除本程式的專用任務與 `schedule.json`，保留執行檔副本和最近紀錄。刪除任務不會中止已在執行的動作；已送出的關機要求無法保證取消。[Microsoft `schtasks delete` 文件](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/schtasks-delete)

若要完整移除，先在程式內停用每週排程、關閉程式，再刪除上述本機目錄與原始執行檔。不要只刪除下載的執行檔，因為每週任務使用的是本機副本。

## 關機行為與限制

- 每週排程需要目前使用者已登入、電腦保持清醒。任務設定為不喚醒電腦（`WakeToRun=false`），不補跑錯過的觸發（`StartWhenAvailable=false`）。
- 單次倒數需保持程式開啟；跨睡眠或休眠的倒數行為不保證。
- 真實模式呼叫 `shutdown.exe /s /t 0`，不使用 `/f` 強制結束應用程式。未儲存的文件或其他應用程式可能阻擋關機；請先儲存工作。[Microsoft `shutdown` 文件](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/shutdown)
- 排程入口會核對已儲存的模式、星期與預定時刻後 0–119 秒的執行範圍，並以執行紀錄防止同一預定時刻重複執行。這些檢查不代表會補跑漏掉的排程。
- Windows 接受關機要求不代表已完成關機；執行紀錄描述的是要求結果。
- 每週排程與真正關機僅支援 Windows。Linux 可用於編譯與單元測試。
- 自動化測試不註冊真正關機任務，也不執行真正關機；通過測試不代表已完成實機關機驗證。

## 開發與驗證

主要套件為 `eframe 0.33.3`（Glow、AccessKit）、`chrono 0.4` 與 Serde。

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
```

GitHub Actions 在 Windows 執行格式、Clippy、單元測試、release 建置與命令列輸出檢查。成功後可從該次 workflow 的 artifact 下載執行檔，不自動建立 GitHub Release。
