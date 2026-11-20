# R Auto Shutdown

繁體中文桌面關機計時器，支援 Windows、macOS 和 Linux。可設定單次倒數，Windows 另支援每週背景排程與系統通知。首次使用預設為模擬模式，啟用真正關機前需確認。

A desktop shutdown timer for Windows, macOS, and Linux, with advance reminders, simulation mode, and Windows weekly scheduling.

## 功能

| 模式 | 平台 | 設定方式 | 關閉主視窗後 |
| --- | --- | --- | --- |
| 單次倒數 | Windows、macOS、Linux | 1–10,080 分鐘，或下一個本機 `HH:MM` | 取消本次倒數 |
| 每週排程 | Windows | 多選星期與 `HH:MM`，可快速選取每天或平日 | 仍會啟動獨立通知，倒數到期後執行 |

- 每週預設為每天 `23:30`、模擬模式，按下「儲存並啟用排程」後才生效。
- 模擬排程會實際註冊 Windows 任務並顯示通知，到期只保存結果，不呼叫關機。
- 編輯中的預覽與已生效設定分別顯示；變更星期、時間或模式後，需重新儲存。
- 真實模式在開始單次倒數或儲存每週排程前，會顯示確認畫面。
- 單次指定時刻已過或等於目前分鐘時，排到明天。
- 單次啟動至少需預留 30 秒；指定時刻太近時，會要求重新設定。

### 關機前通知

進入最後五分鐘時，獨立倒數視窗會置頂，顯示預定時間與模擬／真實模式。單次倒數在五分鐘內時會立即顯示。Windows 同時送出系統通知，橫幅顯示依 Windows 通知與「勿擾」設定而定。macOS 和 Linux 使用應用程式的倒數視窗。Linux 的原生 Wayland 由桌面環境控制視窗層級，置頂與自動還原可能無法使用。

| 通知操作 | 結果 |
| --- | --- |
| 延後 10／30 分鐘 | 累加至目前預定時間；若已到期但尚未執行，則從現在起算 |
| 連續延後 | 每次繼續累加，剩餘時間最多 7 天 |
| 延後後 | 保持通知可見，降為普通視窗；再次剩五分鐘時重新置頂提示 |
| 縮小通知視窗 | 倒數會繼續。還原後仍可延後或取消 |
| 取消本次關機 | 停止這次倒數，每週固定規則不變 |
| 關閉通知視窗 | 等同取消本次關機 |

通知無人操作時，仍會在目前預定時間執行。每週通知由獨立程式執行，主視窗關閉不會取消它；單次通知依附主程式，關閉主視窗會取消單次倒數。

倒數到期時，程式會檢查通知進入顯示狀態是否已滿 30 秒。若通知太晚開啟、未提供足夠操作時間，會取消本次並顯示原因，避免通知剛出現就關機。

關機執行失敗，或取消／完成的紀錄無法保存時，通知會留下錯誤說明，倒數已停止，也不會自行重試關機。已結束的錯誤視窗不會阻擋下一個每週週期啟動通知。

## 執行

已通過建置的版本可從 [GitHub Actions](https://github.com/ntustRay/rust-auto-shutdown/actions/workflows/ci.yml) 對應執行紀錄的 Artifacts 下載。選擇平台與架構，Windows 解壓後執行 `r-auto-shutdown.exe`。macOS 和 Linux 的下載檔須再解開內含的 `.tar.gz`，在桌面工作階段開啟終端機並執行 `./r-auto-shutdown`。

從原始碼執行需要 Rust 1.88 以上。Windows MSVC 工具鏈另需 C++ 建置工具。Linux 需安裝 `fontconfig` 和繁體中文字型，例如 Ubuntu 的 `fonts-noto-cjk`。Linux 的建置套件請參考官方文件。[eframe 建置說明](https://github.com/emilk/egui/blob/0.33.3/crates/eframe/README.md)

```sh
cargo run --release
```

建置結果位於 `target/release`。新版改用 `r-auto-shutdown` 檔名，原有 Windows 下載檔或桌面捷徑可改指向新版。既有 Windows 每週設定仍沿用原資料目錄與任務名稱。

### Windows 每週排程

1. 在「每週排程」選擇至少一個星期，輸入 24 小時制時間，例如 `23:30`。
2. 先保留模擬模式，按下「儲存並啟用排程」。確認畫面顯示已生效設定。
3. 原定關機前五分鐘會出現通知，可測試延後、取消與關閉操作；模擬模式不會關機。
4. 需要真正關機時，解除模擬模式，重新儲存並完成確認。
5. 要停止後續執行，按下「停用每週排程」。關閉視窗不會停用每週排程。

重新開啟程式會載入已儲存的每週設定，包括其模擬／真實模式。

每週排程在關機前五分鐘啟動通知。例如星期一 `00:02` 的關機排程，會在星期日 `23:57` 通知。如果在最後五分鐘才儲存，程式會立即開啟本次通知。如果剩餘時間不足 30 秒，本次會順延，保留至少 30 秒供你操作。固定的每週時間仍以設定為準，本次實際倒數與延後時間請看通知視窗。

從 0.1 或 0.2 更新時，請至少開啟一次新版主程式。程式會嘗試更新背景執行檔與通知方式，保留原星期、時間與模式。若提示更新失敗，請關閉現有提醒後重新儲存排程。

### 單次倒數

開啟「單次倒數」，設定分鐘數或指定時刻後啟動。可先用 **10 分鐘模擬倒數**驗證，剩五分鐘時會出現提醒，縮小視窗後仍會走到模擬完成。關閉主視窗或提醒視窗會取消單次倒數。Windows 的單次倒數與每週排程各自運作。

### 命令列參數

| 參數 | 用途 |
| --- | --- |
| 無參數 | 開啟主視窗；Windows 載入已儲存的每週設定 |
| `--dry-run` | 相容啟動旗標；開啟同一視窗，不覆寫已儲存設定或停用既有任務 |
| `--help`、`-h` | 顯示說明並結束 |
| `--version`、`-V` | 顯示版本並結束 |

排程由視窗設定，未知參數或不支援的參數組合以結束碼 `2` 結束。內部 `--scheduled-dry-run`／`--scheduled-shutdown` 入口供工作排程器啟動通知，不是命令列設定排程或直接關機功能。

## Windows 排程的本機保存與停用

每週排程以目前 Windows 使用者的身分執行，不要求或保存 Windows 密碼，也不安裝背景服務。任務使用 `InteractiveToken` 與 `LeastPrivilege`；一般使用者可建立自身任務，但系統政策或權限仍可能拒絕建立。[Microsoft 任務安全性文件](https://learn.microsoft.com/en-us/windows/win32/taskschd/security-contexts-for-running-tasks)

| 項目 | 位置／內容 |
| --- | --- |
| 專用 Windows 任務 | `RustAutoShutdown-<目前使用者 SID>` |
| 執行檔副本 | `%LOCALAPPDATA%\RustAutoShutdown\app.exe` |
| 每週設定 | 同目錄 `schedule.json`：星期一至日、時間、模擬模式 |
| 最近執行紀錄 | 同目錄 `last-run.json`：原定週期、狀態更新時間、模式、結果及訊息 |

儲存時先註冊任務，再提交設定；設定提交失敗時會嘗試復原原有任務。若復原也失敗，程式會顯示需要處理的任務名稱。

相同內容的執行檔副本會略過複製。更新不同版本時，若副本仍被通知程式使用而無法覆寫，會提示先取消或關閉通知，再重新儲存。

停用只刪除本程式的專用任務與 `schedule.json`，保留執行檔副本和最近紀錄。刪除任務不會中止已開啟的通知，請在通知內取消本次；已送出的關機要求無法保證取消。[Microsoft `schtasks delete` 文件](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/schtasks-delete)

紀錄會區分等待通知操作、模擬完成、已取消與執行結果。延後不修改 `schedule.json` 或該次原定週期識別；取消也不會刪除每週規則。紀錄採暫存檔與原子替換，若既有紀錄無法解析，程式會拒絕啟動該次通知以避免重複執行。

若要完整移除，先在程式內停用每週排程，取消所有已開啟通知並關閉程式，再刪除上述本機目錄與原始執行檔。不要只刪除下載的執行檔，因為每週任務使用的是本機副本。

## 關機行為與限制

- 每週排程需要目前使用者已登入、電腦保持清醒。任務設定為不喚醒電腦（`WakeToRun=false`），不補跑錯過的觸發（`StartWhenAvailable=false`）。
- 單次倒數需保持程式開啟；跨睡眠或休眠的倒數行為不保證。
- Windows 呼叫 `shutdown.exe /s /t 0`，讓應用程式有機會阻擋關機。請先儲存工作。[Microsoft `shutdown` 文件](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/shutdown)
- macOS 透過 `osascript` 向 System Events 送出關機要求，需要已登入的圖形桌面。macOS 可能在第一次送出要求時請你核准自動化權限，可在「系統設定 → 隱私權與安全性 → 自動化」查看。[Apple 自動化權限說明](https://support.apple.com/zh-tw/guide/mac-help/mchl07817563/mac)
- Linux 透過 `busctl` 呼叫 systemd-logind 的 `PowerOff(false)`，需要可用的 logind 與當前使用者的關機權限。系統政策、其他登入工作階段或關機抑制鎖可拒絕要求。[systemd-logind D-Bus API](https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html)
- 一般 Windows 任務須在原通知點後兩分鐘內啟動。儲存時的即時提醒會另外核對本次的星期、時間與模式。兩種入口共用當次執行紀錄，避免同一個週期重複通知。
- 執行紀錄描述系統是否接受關機要求。跨平台 CI 只執行測試與建置，實機關機需在各平台的桌面工作階段驗證。

## 開發與驗證

主要套件為 `eframe 0.33.3`（Glow、AccessKit）、`chrono 0.4` 與 Serde。

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
```

GitHub Actions 在 Windows x64、Linux x64、macOS Apple silicon 與 macOS Intel 執行格式、Clippy、單元測試、release 建置與命令列輸出檢查。
