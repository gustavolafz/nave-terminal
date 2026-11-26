# Nave Terminal for PowerShell
$NaveScript = Join-Path $env:USERPROFILE ".config\nave\nave.ps1"
if (Test-Path $NaveScript) { . $NaveScript }
