# Documento de Requisitos e Arquitetura: Avell Custom Control (Port Linux)
**Versão:** 2.0  
**Status:** Draft para Revisão  
**Hardware Alvo:** Avell Storm 470 (Chassis TongFang)

---

## 1. Visão Geral do Projeto

Desenvolvimento de uma GUI nativa para Linux com controle total sobre os recursos de hardware do Avell Storm 470, substituindo a solução CLI existente. O escopo cobre monitoramento de telemetria, perfis de energia, controle de refrigeração, iluminação RGB e gestão de bateria — replicando e expandindo as funcionalidades do software OEM original.

---

## 2. Arquitetura do Sistema

O sistema adota uma arquitetura **cliente-servidor local** com separação explícita de privilégios e canais de comunicação distintos por padrão de acesso.

```
┌─────────────────────────────────────────────┐
│              GUI (User Space)               │
│         Qt6 Frontend Application            │
└──────────┬──────────────────┬───────────────┘
           │                  │
     D-Bus (Comandos)   Unix Socket (Telemetria)
     one-shot, async    stream, 500ms batch
           │                  │
┌──────────▼──────────────────▼───────────────┐
│           Daemon (avell-controld)            │
│         Rust · systemd · root               │
├─────────────────────────────────────────────┤
│  Kernel Interfaces: hwmon · WMI · EC · RAPL │
└─────────────────────────────────────────────┘
```

### 2.1. Daemon (avell-controld)

- Processo em background gerenciado via **systemd** com privilégios `root`.
- Responsável por toda comunicação direta com o kernel: módulos ACPI, WMI, Embedded Controller (EC) e hwmon.
- Expõe **dois canais IPC** com responsabilidades distintas:

### 2.2. Canais IPC

| Canal | Protocolo | Uso | Justificativa |
|---|---|---|---|
| **Comandos** | D-Bus (system bus) | Mudança de perfil, set RGB, set fan mode, set battery limit | Integração com PolicyKit para autorização sem senha; adequado para operações one-shot |
| **Telemetria** | Unix Domain Socket (`/run/avell-controld/telemetry.sock`) | Streaming de métricas de CPU, GPU, RAM, fans | Latência mínima; evita overhead do D-Bus para payloads frequentes |

> **Rationale:** D-Bus introduz overhead de serialização inadequado para polling de telemetria em 32 cores a ~1Hz. O Unix Socket permite payloads binários compactos com latência sub-milissegundo.

### 2.3. Frontend (GUI)

- Aplicação Qt6 executada em **user space**, sem acesso direto ao hardware.
- Comunicação exclusiva via IPC com o daemon.
- Thread dedicada para consumo do socket de telemetria, desacoplada do loop principal de render.

---

## 3. Requisitos da Interface Gráfica (Frontend)

### 3.1. Dashboard — Monitoramento em Tempo Real

**Métricas exibidas:**
- **CPU:** Load % e temperatura por core (agregado + individual para i9-14900HX: 24 cores / 32 threads).
- **GPU:** Usage %, temperatura e VRAM utilizada (via NVML).
- **Sistema:** RAM utilizada/total e uso de SSD.

**Requisito de threading:**
- Thread de telemetria separada consome o Unix Socket e publica via `QMetaObject::invokeMethod` (thread-safe) para o modelo de dados da UI.
- Payload agregado a cada **500ms** (configurável): evita UI freezing e reduz context-switches.
- A thread principal da UI nunca bloqueia em I/O.

**Formato do payload de telemetria (JSON compacto sobre o socket):**
```json
{
  "ts": 1718000000,
  "cpu": { "load": [12.5, 8.3, "..."], "temp_pkg": 71.0 },
  "gpu": { "usage": 45.0, "temp": 68.0, "vram_used_mb": 3200 },
  "ram_used_mb": 18432,
  "fans": [{ "id": 0, "rpm": 2400 }, { "id": 1, "rpm": 2380 }]
}
```

### 3.2. Modos de Desempenho

- **Perfis disponíveis:** Silencioso · Equilibrado · Turbo
- Botões de seleção com indicação visual clara do perfil ativo.
- Atualização reativa do estado via sinal D-Bus `ProfileChanged` emitido pelo daemon — garante sincronização mesmo quando a troca ocorre por atalho de teclado externo.

### 3.3. Controle de Refrigeração (Fan Control)

**Curva Customizada:**
- Gráfico cartesiano interativo (Temperatura °C × RPM) com pontos de controle arrastáveis.
- Renderizado via **QtCharts** com drag nativo nos pontos de controle.

**Estrutura de dados da curva:**
```json
{
  "fan_id": 0,
  "mode": "custom",
  "curve": [
    { "temp_c": 40, "rpm": 800 },
    { "temp_c": 60, "rpm": 1800 },
    { "temp_c": 75, "rpm": 3200 },
    { "temp_c": 90, "rpm": 4800 }
  ]
}
```

**Restrições de validação (UI deve impor):**
- Mínimo de **2** e máximo de **8** pontos de controle.
- RPM mínimo: **600 RPM** (limite térmico de segurança — validar contra spec do EC do Storm 470).
- Temperatura máxima configurável: **95°C** (hard limit; acima disso o EC assume controle automaticamente).
- Pontos devem ser estritamente monotônicos em temperatura e RPM.

**Modos rápidos:**
- **Automático:** Delega controle ao BIOS/EC.
- **Fan Boost:** Força 100% de RPM em ambas as ventoinhas.

### 3.4. Iluminação RGB do Teclado

- **Color Picker:** Roda de cores HSV + campo de entrada hexadecimal (`#RRGGBB`).
- **Brilho:** Slider 0–100% (mapeado para o range aceito pelo driver).
- **Efeitos:** Menu suspenso — Estático · Respiração (Breathing) · Onda (Wave) · Reativo.

### 3.5. Gestão de Bateria

- Seletores de limite máximo de carga: **60% · 80% · 100%**
- Exibição do status atual lido de `charge_control_end_threshold`.
- Warning visual quando limite = 100% (modo plugged-in permanente).

---

## 4. Requisitos de Integração de Hardware (Backend)

### 4.1. Matriz de Interfaces do Kernel

| Componente | Interface Linux | Mecanismo | Observação |
|---|---|---|---|
| Sensores Térmicos CPU | `/sys/class/hwmon/hwmon*/temp*_input` | Leitura direta via sysfs | Identificar hwmon correto pelo `name` file |
| Sensores GPU | NVML (`libnvidia-ml.so`) | Biblioteca NVIDIA | Fallback: `/sys/class/hwmon/` para GPU temp |
| Perfis de Energia | `/sys/class/powercap/intel-rapl/` | Escrita em `constraint_0_power_limit_uw` (PL1) e `constraint_1_power_limit_uw` (PL2) | Requer desabilitar EPP; ver §4.2 |
| Governadores CPU | `/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor` + `energy_performance_preference` | `cpupower` ou escrita direta | Necessário para Turbo ter efeito real |
| Iluminação RGB | A definir após §4.3 (Hardware Compatibility Probe) | `tuxedo-keyboard` ou EC direto | Driver pode não suportar PCI/USB ID do Storm 470 |
| Ventoinhas (EC) | A definir após §4.3 | Módulo ACPI WMI ou `ec_sys` | Acesso direto a portas I/O bloqueado no kernel ≥5.x com `CONFIG_STRICT_DEVMEM` |
| Limite de Bateria | `/sys/class/power_supply/BAT0/charge_control_end_threshold` | Escrita direta via sysfs | Verificar suporte do ACPI BIOS do Storm 470 |

### 4.2. Perfis de Energia — Configuração Completa

Cada perfil deve ajustar **simultaneamente**:

| Parâmetro | Silencioso | Equilibrado | Turbo |
|---|---|---|---|
| PL1 — Long Duration (W) | 35 | 55 | 95 |
| PL2 — Short Duration / MTP (W) | 55 | 100 | 157 |
| Tau (s) | 28 | 56 | 56 |
| Governador | `powersave` | `powersave` | `performance` |
| EPP | `power` | `balance_performance` | `performance` |
| Fan Mode | Automático | Automático | Custom / Boost |

> **Base de referência (Intel ARK oficial — i9-14900HX):**
> - **Processor Base Power (PL1):** 55W
> - **Maximum Turbo Power (PL2):** 157W
> - **Minimum Assured Power:** 45W
> - **TJunction:** 100°C
>
> Os perfis acima derivam desses limites: Silencioso fica abaixo do PL1 base para priorizar temperatura e ruído; Equilibrado respeita o envelope Intel; Turbo usa o teto de MTP (157W) com Tau completo de 56s.
>
> **Budget total CPU+GPU em modo Turbo:** i9 ~95W (PL1 sustentado) + RTX 4070 140W TGP = ~235W. Verificar se o adaptador AC do Storm 470 suporta este envelope antes de habilitar PL1>55W em modo plugado.
>
> ⚠️ Valores de PL1 acima de 55W são **OEM-tuned** e requerem calibração térmica obrigatória via `stress-ng --cpu 24 --timeout 300` + `turbostat --interval 1` para confirmar ausência de thermal throttling sustentado.

### 4.3. Hardware Compatibility Probe (Pré-requisito de Implementação)

> ⚠️ **Nenhuma implementação de backend deve iniciar sem executar e documentar os resultados deste probe.**

Executar no hardware alvo e registrar output completo:

```bash
# 1. Inventário de sensores hwmon
for d in /sys/class/hwmon/hwmon*/; do
  echo "--- $(cat $d/name) ---"
  ls $d | grep -E 'temp|fan|in'
done

# 2. Dispositivos WMI disponíveis
ls /sys/bus/wmi/devices/
cat /proc/acpi/wmi/guid 2>/dev/null

# 3. Verificar suporte ao tuxedo-keyboard
modinfo tuxedo_keyboard 2>/dev/null | grep -E 'alias|version|filename'
ls /sys/class/leds/ | grep -i kbd

# 4. Verificar EC via ec_sys
modprobe ec_sys write_support=1 2>/dev/null
ls /sys/kernel/debug/ec/ 2>/dev/null

# 5. Verificar battery threshold
cat /sys/class/power_supply/BAT0/charge_control_end_threshold 2>/dev/null

# 6. RAPL domains disponíveis
ls /sys/class/powercap/intel-rapl/

# 7. PCI/USB IDs relevantes (para matching do driver RGB)
lsusb | grep -iE 'keyboard|ite|tongfang'
lspci | grep -iE 'smc|ec'
```

Os resultados determinam:
- Se `tuxedo-keyboard` cobre o dispositivo ou se é necessário DKMS customizado / EC direto.
- O mecanismo exato de controle das ventoinhas (WMI GUID, registradores EC ou módulo ACPI).
- A disponibilidade real do `charge_control_end_threshold`.

---

## 5. Stack Tecnológico

### Decisão

| Camada | Tecnologia | Justificativa |
|---|---|---|
| **Daemon** | **Rust** | Zero-overhead, segurança de memória sem GC, FFI nativo para NVML/ACPI, ecossistema `sysfs`/`nix` maduro |
| **Frontend** | **Qt6 / C++** | `QtCharts` com drag nativo para fan curve; performance de render superior ao GTK4 para gráficos interativos; bindings Rust via `cxx-qt` se desejado |
| **IPC Telemetria** | **Unix Domain Socket** | Latência mínima para streaming contínuo |
| **IPC Comandos** | **D-Bus (zbus / Rust)** | Integração com PolicyKit; padrão systemd |
| **Build** | **Cargo + CMake** | Daemon via Cargo; Qt frontend via CMake |
| **Distribuição** | **DKMS + systemd unit + .deb/.rpm** | Instalação limpa em distros-alvo (Ubuntu/Fedora) |

> **Opção Tauri descartada:** overhead de WebView (Chromium embedded) é injustificável para uma tool de controle de sistema. Penalidade de memória de ~150MB+ é inaceitável no contexto.

> **Opção GTK4 descartada:** ausência de widget de gráfico interativo com drag nativo equivalente ao `QtCharts`. Implementar do zero em Cairo seria custo de desenvolvimento desproporcional.

---

## 6. Fases de Implementação

| Fase | Entregável | Pré-requisito |
|---|---|---|
| **0 — Probe** | Relatório de compatibilidade de hardware (§4.3) | Hardware físico disponível |
| **1 — Daemon Core** | Leitura de telemetria + Unix Socket | Fase 0 concluída |
| **2 — D-Bus Interface** | Perfis de energia + battery limit via D-Bus | Fase 1 |
| **3 — Fan Control** | EC/WMI fan curve + modos rápidos | Fase 0 + 1 |
| **4 — RGB** | Driver RGB funcional (tuxedo ou EC direto) | Fase 0 + 2 |
| **5 — Frontend** | GUI Qt6 completa integrada ao daemon | Fases 1–4 |
| **6 — Packaging** | DKMS + systemd unit + pacotes .deb/.rpm | Fase 5 |

---

## 7. Riscos e Mitigações

| Risco | Probabilidade | Mitigação |
|---|---|---|
| `tuxedo-keyboard` não suporta PCI/USB ID do Storm 470 | Alta | Probe em §4.3; fallback para acesso EC direto via registradores mapeados |
| Acesso EC bloqueado por `CONFIG_STRICT_DEVMEM` | Média | DKMS com módulo ACPI customizado; mapear GUIDs WMI disponíveis |
| `charge_control_end_threshold` não suportado pelo BIOS | Baixa | Probe valida; fallback via WMI se GUID de battery disponível |
| Regressão de kernel quebra interface sysfs | Baixa | CI com teste de smoke em kernel LTS + current |

---

*Documento gerado a partir da v1.0 com revisão arquitetural. Próxima ação: executar Hardware Compatibility Probe (§4.3) e registrar resultados antes de iniciar Fase 1.*
