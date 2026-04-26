# Windows tray panic 修補備忘錄

## 背景

這份文件記錄 `dev/personal` 分支上的 Windows tray 崩潰修補。目的是讓未來追 upstream 更新時，可以快速判斷要保留本地修補、改用 upstream 的修補，或直接用 upstream 來源重新編譯。

原始崩潰指紋來自使用者機器上的桌面版日誌：

- `crash.log` 出現 `assertion failed: flush_paint_messages(None, &subclass_input.event_loop_runner)`。
- panic 位置在 `tao-0.34.4/src/platform_impl/windows/event_loop.rs`。
- backtrace 包含 `tray_icon::platform_impl::platform::tray_proc`。
- 事故表面集中在 Windows system tray / hidden window / Tao event loop，而不是翻譯請求本身。

## 本地修補

目前已採用 dependency-first 修補，將 `src-tauri/Cargo.lock` 中的 `tray-icon` 從 `0.21.1` 更新到 `0.21.3`。

這個修補對應的本地提交：

- `ac325a2 deps: update tray icon for windows stability`

後續又將 upstream `v0.6.14` release 變更套入本地分支，並同步本地版本 metadata：

- `ce46412 chore: sync version to 0.6.14`
- `package.json` -> `0.6.14`
- `src-tauri/Cargo.toml` -> `0.6.14`
- `src-tauri/Cargo.lock` app package -> `0.6.14`
- `src-tauri/tauri.conf.json` -> `0.6.14`

使用者實機觀察一週後，沒有再次發生 app 自動退出或同一組 tray panic 指紋。

## 目前打包結果

本地 `0.6.14` Windows build 已產生下列產物：

- `src-tauri/target/release/app.exe`
- `src-tauri/target/release/bundle/nsis/NextAI Translator_0.6.14_x64-setup.exe`

`pnpm build-tauri` 會在最後因缺少 `TAURI_SIGNING_PRIVATE_KEY` 回傳非零狀態；在本地測試用途上，只要 release exe 與 NSIS installer 已產生，代表可安裝版本已完成。這個錯誤只表示 updater signing 私鑰沒有設定。

## 未來追 upstream 更新時

先抓最新 upstream：

```powershell
git fetch upstream --tags
```

檢查 upstream release 或 main 是否已包含 tray panic 相關修補：

```powershell
git log --oneline --decorate upstream/main -- src-tauri/Cargo.lock src-tauri/Cargo.toml
git diff -- src-tauri/Cargo.lock
```

需要特別確認的點：

- upstream 是否已經不再鎖定會觸發問題的 `tray-icon 0.21.1`。
- upstream 是否已更新到包含 tray panic hotfix 的 `tray-icon 0.21.2` 或更高版本。
- upstream 是否改動了 Tauri / Tao / tray-icon 相關依賴，使本地 `Cargo.lock` 補丁可以移除。
- upstream 是否在 `src-tauri/src/tray.rs` 或 `src-tauri/src/main.rs` 修改 tray 建立或重新整理邏輯。

如果 upstream 已經包含同等或更新的 tray 修補，可以考慮移除本地 dependency-only 補丁，改以 upstream 來源為基底重新編譯。如果 upstream 尚未包含修補，追版本時要保留 `tray-icon >= 0.21.3` 的 lock state。

## 判斷是否可以直接使用 upstream 來源

直接使用 upstream 來源編譯前，至少要滿足以下條件：

- Windows build 的 `Cargo.lock` 沒有回到 `tray-icon 0.21.1`。
- 實機日誌沒有再次出現 `flush_paint_messages` 與 `tray_proc` 同時存在的 panic 指紋。
- tray smoke pass 正常：啟動 app、關閉主視窗讓它進入背景、從 tray 重新叫回主視窗。
- `pnpm test` 或 `pnpm exec vitest run --reporter=verbose` 通過。
- `pnpm test:e2e` 通過。
- `pnpm build-tauri` 至少產生 release exe 與 NSIS installer。

如果上述條件成立，就可以把本地分支往 upstream 更新，並用 upstream 來源加上當前版本 metadata 重新編譯。

## OpenSpec 後續狀態

OpenSpec change `fix-windows-tray-panic` 目前保留兩個條件式後續任務：

- `3.2` 只有在 dependency-first build 再次出現同一組 `flush_paint_messages` / `tray_proc` crash family 時，才實作本地 tray refresh reduction。
- `3.3` 只有在 `3.2` 實際抽出純 tray refresh 決策邏輯後，才新增 Rust unit tests。

目前因為一週觀察沒有再次退出，這兩個任務維持未完成狀態是預期行為。
