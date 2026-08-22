# Updater signing setup wizard (PowerShell 7+)
# Walks you through generating the Tauri updater signing keypair, storing it
# in GitHub Actions secrets, and patching the public key into tauri.conf.json.
# The private key and password never leave your terminal / GitHub secrets.
#
# Run from the repo root:  pwsh -File scripts/setup-updater-signing.ps1

$ErrorActionPreference = 'Stop'

# This clone has two remotes (origin + upstream); pin every gh call to the
# fork so secrets never land on the wrong repository.
$env:GH_REPO = 'zeta987/nextai-translator'

$KeyFile  = Join-Path $env:USERPROFILE '.tauri\nextai-translator-fork.key'
$PubFile  = "$KeyFile.pub"
$ConfFile = 'src-tauri/tauri.conf.json'
$TotalStages = 4
$script:StageIndex = 0
$script:Skipped = @()

function Show-Stage([string]$Title) {
    Clear-Host
    $script:StageIndex++
    Write-Host ""
    Write-Host ("▸ Stage {0}/{1} · {2}" -f $script:StageIndex, $TotalStages, $Title) -ForegroundColor Blue
}
function Say([string]$Msg)  { Write-Host "  $Msg" }
function Note([string]$Msg) { Write-Host "  $Msg" -ForegroundColor DarkGray }
function Warn([string]$Msg) { Write-Host "  ⚠ $Msg" -ForegroundColor Yellow }
function Ok([string]$Msg)   { Write-Host "  ✓ $Msg" -ForegroundColor Green }
function Pause-Wizard([string]$Msg = 'Press Enter to continue') {
    Read-Host "  $Msg" | Out-Null
}
function Confirm-Wizard([string]$Question) {
    $reply = Read-Host "  ? $Question [y/N]"
    return $reply -match '^[Yy]'
}

# ── Preflight ──────────────────────────────────────────────────────────────
foreach ($tool in 'pnpm', 'gh') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Warn "'$tool' not found in PATH — install it first, then re-run."
        exit 1
    }
}
if (-not (Test-Path $ConfFile)) {
    Warn "Run this from the repo root (cannot find $ConfFile)."
    exit 1
}

Clear-Host
Write-Host ""
Write-Host "  NextAI Translator fork — updater signing setup" -ForegroundColor Blue
Note "$TotalStages stages. Stop any time with Ctrl-C and re-run later."
Pause-Wizard "Ready to start?"

# ── Stage 1: generate the signing keypair ──────────────────────────────────
Show-Stage "Generate the Tauri updater signing keypair"
Say "The fork signs its own update artifacts; upstream's key cannot be reused."
Say "The tauri CLI will prompt you for a password — pick one and remember it."
Note "Key file: $KeyFile"
if (Test-Path $KeyFile) {
    Warn "A key file already exists at $KeyFile."
    if (Confirm-Wizard "Keep the existing keypair and skip generation?") {
        Say "Keeping the existing keypair."
    }
    else {
        Warn "Overwriting REVOKES the old key: apps built with the old pubkey"
        Warn "will refuse updates signed by the new key."
        if (Confirm-Wizard "Really overwrite the existing keypair?") {
            pnpm tauri signer generate -w $KeyFile --force
        }
        else {
            Say "Keeping the existing keypair."
        }
    }
}
else {
    New-Item -ItemType Directory -Force (Split-Path $KeyFile) | Out-Null
    pnpm tauri signer generate -w $KeyFile
}
if (-not ((Test-Path $KeyFile) -and (Test-Path $PubFile))) {
    Warn "Key files not found — generation failed?"
    exit 1
}
Ok "Keypair ready."
Pause-Wizard

# ── Stage 2: store the private key + password as GitHub secrets ────────────
Show-Stage "Store the private key in GitHub Actions secrets"
Say "The Fork Release workflow reads these two repository secrets:"
Note "TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
Get-Content -Raw $KeyFile | gh secret set TAURI_SIGNING_PRIVATE_KEY
if ($LASTEXITCODE -eq 0) { Ok "set GitHub secret TAURI_SIGNING_PRIVATE_KEY" }
else {
    Warn "failed to set TAURI_SIGNING_PRIVATE_KEY - set it later"
    $script:Skipped += 'TAURI_SIGNING_PRIVATE_KEY'
}
$password = Read-Host "  Signing key password (hidden)" -MaskInput
$password | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
if ($LASTEXITCODE -eq 0) { Ok "set GitHub secret TAURI_SIGNING_PRIVATE_KEY_PASSWORD" }
else {
    Warn "failed to set TAURI_SIGNING_PRIVATE_KEY_PASSWORD - set it later"
    $script:Skipped += 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD'
}
$password = $null
Pause-Wizard

# ── Stage 3: patch the public key into tauri.conf.json ─────────────────────
Show-Stage "Patch the public key into $ConfFile"
$pubkey = (Get-Content -Raw $PubFile) -replace '[\r\n]', ''
if ([string]::IsNullOrWhiteSpace($pubkey)) {
    Warn "Public key file is empty?"
    exit 1
}
$conf = Get-Content -Raw $ConfFile
$patched = $conf -replace '"pubkey": "[^"]*"', ('"pubkey": "' + $pubkey + '"')
Set-Content -Path $ConfFile -Value $patched -NoNewline
if ((Get-Content -Raw $ConfFile).Contains($pubkey)) {
    Ok "pubkey updated in $ConfFile."
}
else {
    Warn "pubkey not found after patch — edit $ConfFile manually."
    $script:Skipped += "pubkey in $ConfFile (paste contents of $PubFile)"
}
Note "Commit this change afterwards — the app verifies updates against it."
Pause-Wizard

# ── Stage 4: verify + back up ──────────────────────────────────────────────
Show-Stage "Verify and back up"
Say "Repository secrets now on the fork:"
gh secret list
Write-Host ""
Warn "BACK UP $KeyFile (and its password) somewhere safe."
Warn "If it is lost, installed apps can NEVER accept another update"
Warn "and every user must reinstall manually."
Say "Copy it to your password manager or an encrypted backup now."
Pause-Wizard "Done backing up? Press Enter to finish."

Clear-Host
Write-Host ""
Ok "Setup complete"
if ($script:Skipped.Count -gt 0) {
    Warn "still to do by hand:"
    $script:Skipped | ForEach-Object { Note "  - $_" }
}
Write-Host ""
