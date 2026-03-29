# 除錯日誌與崩潰捕獲

NextAI Translator 內建日誌系統，用於診斷 Windows 背景運行時的無徵兆崩潰。

## 日誌檔案位置

### Windows

| 檔案 | 路徑 | 用途 |
|------|------|------|
| `app.log` | `%APPDATA%\xyz.yetone.apps.openai-translator\logs\app.log` | 一般應用程式日誌（生命週期事件、警告、錯誤） |
| `crash.log` | `%APPDATA%\xyz.yetone.apps.openai-translator\logs\crash.log` | Panic 崩潰報告，含完整 backtrace |

當 `app.log` 超過 5 MB 時會自動輪替（`app.log.1`、`app.log.2` 等），保留在同一目錄下。

### macOS

| 檔案 | 路徑 |
|------|------|
| `app.log` | `~/Library/Logs/xyz.yetone.apps.openai-translator/app.log` |
| `crash.log` | `~/Library/Logs/xyz.yetone.apps.openai-translator/crash.log` |

### Linux

| 檔案 | 路徑 |
|------|------|
| `app.log` | `~/.config/xyz.yetone.apps.openai-translator/logs/app.log` |
| `crash.log` | `~/.config/xyz.yetone.apps.openai-translator/logs/crash.log` |

## 從系統匣快速開啟

在系統匣圖示上按右鍵，選擇 **「View Logs」** 即可用檔案管理員開啟日誌目錄。

## 崩潰後的排查步驟

1. 系統匣右鍵 > **View Logs**（或手動前往上述路徑）
2. 先看 **`crash.log`** — 如果有最近的記錄，代表是 Rust panic 導致崩潰。Backtrace 會顯示精確的函式與行號。
3. 再看 **`app.log`** — 捲到最底部，日誌中斷前的最後幾行就是 app 崩潰前正在做的事。
4. 如果 `crash.log` 沒有新記錄且 `app.log` 只是突然中斷，代表 process 是被外部終止的（Windows Update、防毒軟體、記憶體不足等）。

## 記錄內容

- **App 生命週期**：啟動、設定完成、就緒狀態、退出
- **滑鼠鉤子**：config 載入失敗、滑鼠位置取得錯誤
- **更新檢查**：定期檢查結果與失敗
- **IPC 伺服器**：綁定狀態、請求錯誤
- **OCR**：Windows OCR API 錯誤
- **Fetch**：串流請求資訊、中止事件
- **Panic**：執行緒名稱、位置、訊息、完整 backtrace（寫入 `crash.log`）
