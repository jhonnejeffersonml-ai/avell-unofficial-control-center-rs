# Setup — Avell Storm 470 (Linux)

Guia de instalação em produção para o **Avell Storm 470** (chassi TongFang) rodando Ubuntu 22.04+ ou Debian 12+.

---

## Hardware suportado

| Componente | Dispositivo | USB ID |
|---|---|---|
| Teclado RGB | ITE Device 8291 | `048d:600b` |
| Lightbar frontal | ITE Device 8233 | `048d:7001` |
| Perfis de energia | Intel RAPL + sysfs | — |
| Telemetria (CPU/GPU/RAM) | hwmon + nvidia-smi | — |

> Outros modelos com o mesmo chassi TongFang e controladores ITE 8291/8233 são compatíveis.

---

## Pré-requisitos

### Pacotes de sistema (runtime)
```bash
sudo apt install -y libusb-1.0-0 libgcc-s1 policykit-1
```

### Para compilar do código-fonte (build)
```bash
sudo apt install -y build-essential pkg-config libusb-1.0-0-dev
```

### Rust toolchain (build)
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

---

## Instalação rápida (recomendada)

```bash
git clone https://github.com/seu-usuario/avell-unofficial-control-center
cd avell-unofficial-control-center

sudo ./install/install.sh
```

O script faz automaticamente:
- Compila os binários Rust (`aucc` e `aucc-ui`)
- Instala binários em `/usr/local/bin/`
- Instala a regra udev para acesso ao HID sem root
- Instala a polkit policy para `pkexec` sem senha
- Adiciona seu usuário ao grupo `plugdev`

> **Após a instalação:** faça **logout e login** para o grupo `plugdev` ter efeito.

---

## Instalação com binário pré-compilado

Se você já tem os binários compilados (`aucc-rs/target/release/`):

```bash
sudo ./install/install.sh --skip-build
```

---

## Uso

### Interface interativa (TUI)
```bash
pkexec aucc-ui
# ou pelo atalho na raiz do projeto:
./teclado
```

### Linha de comando

```bash
# Telemetria em tempo real (não precisa de root)
aucc --telemetry

# Perfis de energia
pkexec aucc --profile silent      # Silencioso — 35W PL1 / 55W PL2
pkexec aucc --profile balanced    # Equilibrado — 55W PL1 / 100W PL2
pkexec aucc --profile turbo       # Turbo — 95W PL1 / 157W PL2

# TDP manual
pkexec aucc --tdp 45

# Teclado RGB
aucc --color 255 0 128            # cor sólida (R G B)
aucc --effect breathing           # efeito respiração
aucc --effect wave                # onda
aucc --brightness 3               # brilho 1–4
```

---

## O que cada comando precisa

| Função | Precisa de root? | Motivo |
|---|---|---|
| Telemetria (CPU/GPU/RAM) | ❌ Não | leitura de sysfs/hwmon |
| Teclado RGB | ❌ Não¹ | acesso via udev + plugdev |
| Lightbar | ❌ Não¹ | hidraw via udev + plugdev |
| Perfis de energia (RAPL) | ✅ Sim (pkexec) | escrita em `/sys/class/powercap` |
| Governor/EPP | ✅ Sim (pkexec) | escrita em `/sys/devices/system/cpu` |

¹ Requer que o usuário esteja no grupo `plugdev` e que a udev rule esteja instalada.

---

## Verificar instalação

```bash
# Confirmar que os dispositivos estão acessíveis (sem sudo)
ls -la /dev/hidraw*
# Deve mostrar grupo 'plugdev' com modo rw-rw----

# Confirmar grupos do usuário
groups
# Deve incluir 'plugdev'

# Testar telemetria
aucc --telemetry

# Testar perfil (exige pkexec/polkit)
pkexec aucc --profile balanced
```

---

## Limitações conhecidas

| Recurso | Status | Motivo |
|---|---|---|
| LED do botão de perfil | ❌ Não suportado | EC firmware-controlled; WMDE stub no BIOS |
| Controle de ventoinhas | ❌ Não suportado | Sem `fan*_input` em hwmon; sem driver EC |
| Limite de carga da bateria | ❌ Não suportado | `charge_control_end_threshold` ausente no BIOS |

---

## Estrutura do projeto

```
avell-unofficial-control-center/
├── aucc-rs/              # Implementação Rust (principal)
│   ├── src/
│   │   ├── keyboard/    # ITE 8291 — RGB via libusb
│   │   ├── lightbar/    # ITE 8233 — hidraw HIDIOCSFEATURE
│   │   ├── power/       # RAPL + governor + EPP
│   │   ├── telemetry/   # hwmon + nvidia-smi + /proc
│   │   └── ui/          # TUI (ratatui)
│   └── Cargo.toml
├── aucc/                 # Implementação Python (legada)
├── install/
│   ├── install.sh        # Script de instalação
│   ├── 70-avell-hid.rules # Regra udev
│   └── org.avell.aucc.policy # Polkit policy
├── teclado               # Atalho de lançamento
└── SETUP.md              # Este arquivo
```
