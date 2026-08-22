# Fork 發版與同步流程

這份文件記錄 `zeta987/nextai-translator`（個人 fork）的分支策略、發版流程與 upstream 同步方式。決策背景見 `docs/adr/0001-fork-release-branch-strategy.md`。

## 分支策略

- **`main`**：upstream 純鏡像。永遠不放 fork 自己的 commit，讓 GitHub 的 Sync fork 按鈕維持一鍵 fast-forward、零衝突。
- **`dev/personal`**：主要開發分支，也是發版來源。所有 fork 修補（tray-icon 鎖定、crash logging、release pipeline）都在這裡。
- 功能與修復用 `feat/`、`fix/` 分支開發後合回 `dev/personal`。

## Upstream 同步（全手動）

1. GitHub 網頁上按 fork 的 **Sync fork** 按鈕（或 `git fetch upstream && git push origin upstream/main:main`），讓 `main` 跟上 upstream。
2. 要把 upstream 更新帶進 `dev/personal` 時，在本地開分支合併（可請 agent 協助）。每次合併必須重新確認：
   - `Cargo.lock` 的 `tray-icon` 仍然 `>= 0.21.3`（upstream 持續鎖在會閃退的 0.21.1）。
   - crash logging、panic hook、View Logs 沒有被 upstream 重構蓋掉。
   - 驗證清單照 `docs/WINDOWS-TRAY-PANIC-FIX-ZH.md` 執行。

## 發版流程

前置（一次性）：執行 `bash scripts/setup-updater-signing.sh` 產生簽章金鑰、
寫入 GitHub secrets、把公鑰 patch 進 `tauri.conf.json`（記得 commit）。

之後每次發版：

1. 確認 `dev/personal` 上版本 metadata 正確（`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`Cargo.lock`）。
2. 下 tag 並推送：

   ```bash
   git tag -a fork-v0.6.42 -m "release notes here"
   git push origin fork-v0.6.42
   ```

3. `Fork Release` workflow（`.github/workflows/release.yaml`）會自動：
   - 建 draft release
   - 在 ubuntu 上建 renderer、在 windows-latest 上以 MSVC 建 Windows x64 NSIS 安裝檔
   - 由 tauri-action 產生 `latest.json` 與 `.sig` 簽章檔並附到 release
   - 全部成功後把 release 轉為正式發佈

Tag 一律用 `fork-v` 前綴，避免跟 `git fetch upstream --tags` 抓進來的 upstream `v*` tag 同名衝突。

## Updater 行為

- `tauri.conf.json` 的 updater endpoint 指向 **fork** 的
  `https://github.com/zeta987/nextai-translator/releases/latest/download/latest.json`，
  app 只會收到 fork 自己發的更新，upstream 發版不會再蓋掉本地修補。
- 公鑰換成 fork 自己的之後，**舊 build（含 2026-08-22 本機編的 0.6.42）驗不過新簽章**，
  第一個 fork release 需要手動下載安裝一次，之後的更新才會自動走 updater。
- 私鑰只存在於本機 `~/.tauri/nextai-translator-fork.key` 與 GitHub secrets。
  **務必備份**：私鑰遺失的話已安裝的 app 永遠無法再接受自動更新。
