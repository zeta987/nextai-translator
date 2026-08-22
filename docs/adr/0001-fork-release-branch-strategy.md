# ADR-0001: main 作為 upstream 純鏡像，dev/personal 作為發版分支

日期：2026-08-22
狀態：Accepted

## 背景

這個 repo 是 `nextai-translator/nextai-translator` 的個人 fork，永久攜帶 upstream
不收錄的修補（tray-icon 0.21.3 鎖定、crash logging）。fork 需要自己的 release
管道（Windows x64），且 updater 已改指向 fork 的 releases。必須決定 release 從
哪個分支發、upstream 怎麼同步。

## 決策

- `main` 維持 upstream 純鏡像，不放任何 fork commit。
- `dev/personal` 是主要開發分支兼發版來源；release tag（`fork-v*`）從這裡切。
- upstream 同步全手動：Sync fork 按鈕更新 `main`，再由人（可請 agent 協助）
  把 `main` 合進 `dev/personal`。不做自動排程合併。

## 理由

- `main` 無分歧 ⇒ GitHub Sync fork 永遠 fast-forward，同步零成本零風險。
- 若讓 `main` 承載 fork 修改，每次同步都變成手動解衝突，且誤按 Sync fork
  可能觸發 GitHub 的 discard-commits 流程，風險不對稱。
- 合併進 `dev/personal` 需要人工判斷（tray-icon 鎖定重驗、與 upstream 重構
  的意圖合併），自動化合併可能默默弄丟修補，故不排程。

## 後果

- 發版 tag 用 `fork-v*` 前綴，與 upstream `v*` tag 命名空間隔離。
- `dev/personal` 與 `main` 永久分歧是預期狀態，不視為問題。
- 每次追 upstream 的合併照 `docs/WINDOWS-TRAY-PANIC-FIX-ZH.md` 的驗證清單。
