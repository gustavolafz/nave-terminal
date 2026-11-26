<p align="center">
  <img src="assets/readme-banner.png" alt="Nave Terminal" width="100%" />
</p>

<div align="center">

[![Zsh](https://img.shields.io/badge/Zsh-5.9+-7DCFFF?style=flat-square&logo=gnu-bash&logoColor=0E111A)](https://www.zsh.org/)
[![PowerShell](https://img.shields.io/badge/PowerShell-5.1%20%7C%207+-7AA2F7?style=flat-square&logo=powershell&logoColor=0E111A)](https://learn.microsoft.com/powershell/)
[![WSL2](https://img.shields.io/badge/WSL2-Ready-50FA7B?style=flat-square&logo=linux&logoColor=0E111A)](https://learn.microsoft.com/windows/wsl/)
[![Windows Terminal](https://img.shields.io/badge/Windows%20Terminal-HLSL%20Shaders-BB9AF7?style=flat-square)]()
[![License: MIT](https://img.shields.io/badge/License-MIT-E0AF68?style=flat-square)](LICENSE)

</div>

---

<p align="center">
  <img src="assets/nave-demo.gif" alt="Nave Terminal Live Theme & Layout Demo" width="100%" />
</p>

## Destaques

Construído para ser rápido, legível e consistente em qualquer visualizador (Windows Terminal, VS Code, Cursor, Antigravity, ConHost ou SSH), usando apenas Box Drawing (`╭─`, `╰─`, `█`, `░`) e Unicode padrão (`❯`, `±`, `↑`, `↓`, `≡`, `×`):

- **5 paletas + fundo e cursor ao vivo (`OSC 11 / 12`)**: trocar com `nave-theme <nome>` muda instantaneamente o fundo da aba, cursor, prompt, `zsh-syntax-highlighting`, `PSReadLine`, `fzf` e `sysinfo`.
- **4 layouts de prompt (`nave-layout`)**: alterne entre `framed`, `rail` (trilho contínuo até o relógio), `hud` (divisores geométricos `◆ │`) e `compact` (1 linha para split panes).
- **Transient Prompt**: após pressionar Enter, prompts antigos colapsam para uma única linha minimalista (`HH:MM ❯ comando`).
- **Arte TrueColor + Cockpit em tempo real**: `sysinfo` com foguete em gradiente dinâmico, régua divisória `hr [titulo]`, dashboard interativo `nave-hud` e starfield 3D `nave-warp`.
- **Acabamento nativo no Windows Terminal**: 5 esquemas de cor, wallpaper minimalista espacial sob medida e 2 pixel shaders GPU em HLSL (`nave-glow.hlsl` e `nave-crt.hlsl`) alternáveis via `nave-shader`.
- **Toolkit WSL2 + PowerShell**: notificações toast no Windows para comandos longos (`>= 15s`), hub de projetos fuzzy (`p`), radar de portas (`ports` / `killport`), ponte WSL ↔ Windows (`clip`, `paste`, `winopen`, `winip`), Git turbo (`ginfo`, `gbf`, `gwip`, `gbclean`) e Docker cockpit (`dks`, `dkl`, `dksh`).

---

## Showcase principal

<p align="center">
  <img src="assets/hero-showcase.png" alt="Nave Terminal Showcase" width="100%" />
</p>

## Paletas integradas

Troque a qualquer momento com `nave-theme <nome>` — a escolha fica salva em `~/.config/nave/palette` e sincroniza inclusive o fundo da janela (`nave-bg on|off`).

<p align="center">
  <img src="assets/palettes.png" alt="Paletas do Nave Terminal: cyber, emerald, sunset, nord e stealth" width="100%" />
</p>

| Paleta | Comando | Background (OSC 11) | Estilo |
| :--- | :--- | :---: | :--- |
| **Cyber** *(padrão)* | `nave-theme cyber` | `#0E111A` | Cyan elétrico, roxo e verde menta inspirado em Tokyo Night |
| **Emerald** | `nave-theme emerald` | `#0B1510` | Verde neon e teal de alto contraste |
| **Sunset** | `nave-theme sunset` | `#181016` | Laranja quente, coral e magenta |
| **Nord** | `nave-theme nord` | `#232934` | Azul ártico, gelo e tons suaves |
| **Stealth** | `nave-theme stealth` | `#101012` | Monocromático minimalista com foco apenas em alertas e erros |

---

## Arte, Cockpit & Ferramentas

### `sysinfo`, `hr` e `ginfo`
Arte espacial em gradiente TrueColor dinâmico (acompanha a paleta ativa), régua divisória para logs (`hr`) e painel completo do repositório Git:

<p align="center">
  <img src="assets/cockpit-tools.png" alt="sysinfo com arte gradiente, hr e ginfo no Nave Terminal" width="100%" />
</p>

### `nave-hud` (Cockpit em tempo real)
Tela cheia interativa com relógio em blocos gradiente, sparkline histórico de CPU, barras de RAM/SSD e telemetria de rede/Docker (use `1–5` para trocar a paleta ao vivo e `q` para sair). Experimente também `nave-warp` para o starfield 3D.

<p align="center">
  <img src="assets/hud-showcase.png" alt="nave-hud cockpit em tempo real" width="100%" />
</p>

### `nave-help`
Guia rápido embutido no próprio shell:

<p align="center">
  <img src="assets/nave-help.png" alt="Guia rapido nave-help" width="100%" />
</p>

---

## Instalação rápida

### WSL / Linux (Zsh + Oh My Zsh)

```bash
git clone https://github.com/gustavolafz/nave-terminal.git
cd nave-terminal

# Instala tema, toolkit, nave-hud, nave-warp e plugins Zsh
./install.sh

# Opcional: também baixa fzf, zoxide, bat, eza, delta, lazygit, btop, fd e rg em ~/.local/bin (sem sudo)
./install.sh --with-cli
```

Adicione no seu `~/.zshrc` (referência completa em [config/zshrc.example](config/zshrc.example)):

```zsh
export PATH="$HOME/.local/bin:$PATH"
export NAVE_PROJECTS_DIR="$HOME/projects"   # pasta usada pelo comando `p`

ZSH_THEME="nave"
plugins=(git zsh-autosuggestions zsh-syntax-highlighting)
source "$ZSH/oh-my-zsh.sh"

[[ -r "$HOME/.config/nave/toolkit.zsh" ]] && source "$HOME/.config/nave/toolkit.zsh"
```

Recarregue o shell:

```zsh
source ~/.zshrc
nave-help
```

### Extras (PowerShell, Windows Terminal & Git Delta)

<details>
<summary><strong>PowerShell (Windows 5.1 &amp; PowerShell 7+)</strong></summary>

O mesmo visual (4 layouts, 5 paletas com OSC 11/12 ao vivo, arte gradiente no `sysinfo`, `hr`, `ginfo`, `ports`, `killport`, `nave-shader` e `PSReadLine` sincronizado) funciona nativamente no PowerShell:

```powershell
New-Item -ItemType Directory -Path "$env:USERPROFILE\.config\nave" -Force | Out-Null
Copy-Item .\powershell\nave.ps1 "$env:USERPROFILE\.config\nave\nave.ps1"

New-Item -ItemType File -Path $PROFILE -Force | Out-Null
Add-Content -Path $PROFILE -Value "`n. `"$env:USERPROFILE\.config\nave\nave.ps1`""
```
</details>

<details>
<summary><strong>Windows Terminal — 5 Esquemas, Wallpaper &amp; Pixel Shaders HLSL</strong></summary>

1. Copie `windows-terminal/nave-wallpaper.png` e `windows-terminal/shaders/*.hlsl` para `%USERPROFILE%\.config\nave\`.
2. Adicione os 5 esquemas de [windows-terminal/schemes.json](windows-terminal/schemes.json) em `"schemes"` no `settings.json` do Windows Terminal.
3. No seu perfil WSL / PowerShell, configure:

```json
{
  "colorScheme": "Nave Cyber",
  "cursorShape": "bar",
  "padding": "12, 14, 12, 14",
  "opacity": 95,
  "useAcrylic": true,
  "bellStyle": "none",
  "backgroundImage": "C:\\Users\\SEU_USUARIO\\.config\\nave\\nave-wallpaper.png",
  "backgroundImageOpacity": 0.68,
  "backgroundImageStretchMode": "uniform",
  "backgroundImageAlignment": "bottomRight",
  "experimental.pixelShaderPath": "C:\\Users\\SEU_USUARIO\\.config\\nave\\shaders\\nave-glow.hlsl"
}
```

Depois você pode alternar o shader direto pelo terminal com `nave-shader glow`, `nave-shader crt` ou `nave-shader off`.
</details>

<details>
<summary><strong>Git Delta (diff lado a lado colorido)</strong></summary>

Adicione o conteúdo de [config/gitconfig.delta](config/gitconfig.delta) ao seu `~/.gitconfig`:

```ini
[core]
	pager = delta
[interactive]
	diffFilter = delta --color-only
[delta]
	navigate = true
	side-by-side = true
	line-numbers = true
	syntax-theme = TwoDark
	hyperlinks = true
```
</details>

---

## Referência rápida de comandos

| Categoria | Comando / Atalho | O que faz |
| :--- | :--- | :--- |
| **Estilo** | `nave-theme <nome>` | Alterna paleta + fundo/cursor OSC 11 (`cyber`, `emerald`, `sunset`, `nord`, `stealth`) |
| **Estilo** | `nave-layout <modo>` | Muda desenho do prompt (`framed`, `rail`, `hud`, `compact`) |
| **Estilo** | `nave-shader <modo>` | Alterna pixel shader GPU do Windows Terminal (`glow`, `crt`, `off`, `status`) |
| **Estilo** | `nave-transient` / `nave-bg` / `nave-welcome` | Liga/desliga transient prompt, fundo dinâmico e welcome bar |
| **Arte & HUD** | `sysinfo` / `hr [titulo]` | Painel com arte gradiente TrueColor / régua divisória na largura da tela |
| **Arte & HUD** | `nave-hud` / `nave-warp` | Cockpit em tela cheia em tempo real / starfield 3D interativo |
| **Sistema** | `ports` / `killport <porta>` | Radar de portas TCP/UDP abertas e encerramento rápido |
| **Navegação** | `p [projeto]` / `z <termo>` | Hub fuzzy de projetos em `$NAVE_PROJECTS_DIR` e salto inteligente (`zoxide`) |
| **Busca** | `Ctrl+R` / `Ctrl+T` / `Alt+C` | Histórico fuzzy, arquivos com preview (`bat`) e diretórios (`eza`) |
| **Atalho** | `Esc Esc` | Adiciona ou remove `sudo ` no início da linha atual |
| **Git** | `lg` / `ginfo` / `gbf` / `gwip` / `gbclean` | Lazygit, painel do repo, troca de branch fuzzy, WIP e limpeza de branches |
| **Docker** | `dks` / `dkl` / `dksh` | Tabela de containers, logs e shell com seleção interativa via `fzf` |
| **WSL** | `clip` / `paste` / `winopen` / `winip` / `notify` | Clipboard Windows, Explorer, IPs e notificações toast nativas |

---

## Estrutura do repositório

```text
nave-terminal/
├── theme/nave.zsh-theme               # Tema Zsh v3 (paletas OSC 11/12, 4 layouts, transient, toasts)
├── config/toolkit.zsh                 # Arte gradiente, hr, nave-shader, WSL, Git, Docker, projetos
├── config/zshrc.example               # Exemplo limpo de ~/.zshrc
├── config/gitconfig.delta             # Configuração recomendada para git-delta
├── bin/nave-hud                       # Cockpit interativo em tela cheia (relógio + CPU sparkline)
├── bin/nave-warp                      # Starfield 3D interativo sincronizado com a paleta
├── powershell/                        # Versão nativa completa para Windows PowerShell 5.1 e 7+
├── windows-terminal/
│   ├── schemes.json                   # Os 5 esquemas de cor do Windows Terminal
│   ├── nave-wallpaper.png             # Wallpaper minimalista espacial customizado
│   └── shaders/                       # Pixel shaders HLSL (nave-glow.hlsl e nave-crt.hlsl)
├── assets/                            # Banner, GIF animado e capturas em alta resolução
└── install.sh                         # Instalador automático (suporta --with-cli)
```

## Licença

Distribuído sob a licença [MIT](LICENSE).
