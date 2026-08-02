# Persistência da configuração do teclado entre eventos de energia

Data: 2026-08-01
Status: aprovado

## Problema

Ao ligar o notebook na bateria, a iluminação do teclado não acende. Ao conectar
o cabo de força, ela acende. A configuração escolhida pelo usuário não sobrevive
a esses eventos.

## Root cause (verificado)

Evidências coletadas na máquina (Avell Storm 470, ITE 8291 + ITE 8233):

1. `aucc-lightbar-restore.service` roda e conclui com sucesso no boot
   (`enabled=false rgb=(255,255,255) path=/dev/hidraw2`). A lightbar está
   apagada por escolha do usuário — não é o sintoma reportado.
2. `/etc/aucc/lightbar.conf` tem `save_eeprom=true`: a TUI já grava as
   configurações do teclado na EEPROM do ITE 8291 (byte 7 = 0x01).
3. Não existe interface de LED em `/sys/class/leds` para o backlight — o único
   caminho de controle é USB (`rusb`, interface 1, SET_REPORT).
4. Não existe nenhuma persistência de software para o teclado: `config.rs`
   modela apenas a lightbar, e `--lb-restore` só toca no ITE 8233.
5. Com o cabo desconectado (`AC0 online=0`, `BAT0 status=Discharging`),
   `sudo aucc --color white --brightness 4` acendeu o teclado normalmente.
6. Desconectar o cabo apaga o backlight imediatamente.

Conclusão: o EC não bloqueia o backlight na bateria — ele o desliga em eventos
de energia (power-on na bateria e transição AC→bateria), ignorando o estado da
EEPROM. Comandos do userspace funcionam normalmente na bateria. Portanto a
correção é viável em software: persistir o estado do teclado e reaplicá-lo nos
eventos que o EC usa para apagar.

## Solução

### 1. Persistência — `config.rs` vira módulo

Novo arquivo `/etc/aucc/keyboard.conf`, mesmo formato `key=value` do
`lightbar.conf`:

```
mode=mono|halt|valt|effect|off
r,g,b                # cor primária
r2,g2,b2             # cor secundária (modos halt/valt)
brightness=1..4
effect=rainbow|wave|breathing|marquee|reactive|ripple|...
speed=1..10
direction=right|left|up|down
letter=r|o|y|g|b|t|p # sufixo de cor do efeito (opcional)
reactive=true|false
```

`config.rs` (250 linhas, com `load_file` e `parse_file_impl` duplicando o mesmo
parser) passa a `config/{mod,lightbar,keyboard}.rs`, com o parser deduplicado.
Melhoria cirúrgica: é código que a mudança toca de qualquer forma.

### 2. Gravação em toda aplicação

Todo caminho que aplica iluminação no teclado grava o `KeyboardConfig`:
`main.rs::run` (`--color`, `-H`, `-V`, `--style`, `--disable`), `--off` e a TUI.
Não há flag nova — "só muda se eu mudar a configuração" significa que aplicar é
configurar. `--disable` e `--off` gravam `mode=off`, de modo que deixar o
teclado apagado de propósito também persiste.

`--save` (EEPROM) permanece: ajuda no boot em AC, antes do serviço rodar.

### 3. CLI

- `--kb-restore` — aplica `keyboard.conf` (requer root, como os demais comandos
  de teclado).
- `--restore` — restaura lightbar e teclado; é o que o systemd chama.
- `--lb-restore` — mantido para compatibilidade.

### 4. Gatilhos

`aucc-lightbar-restore.service` passa a `aucc-restore.service`, com
`ExecStart=/usr/local/bin/aucc --restore`. O `install` remove a unit antiga
(`systemctl disable --now` + `rm`) antes de escrever a nova.

**`RemainAfterExit` muda de `yes` para `no`.** Com `yes`, o systemd considera o
serviço já ativo e não o re-executa em um novo `SYSTEMD_WANTS` — o gatilho de
troca de energia nunca dispararia. Esse é um requisito, não um detalhe de
estilo.

Regra udev nova, para a transição AC↔bateria:

```
SUBSYSTEM=="power_supply", ACTION=="change", ATTR{type}=="Mains",
    TAG+="systemd", ENV{SYSTEMD_WANTS}+="aucc-restore.service"
```

Adicionalmente, `SYSTEMD_WANTS` no dispositivo USB do teclado (048d:600b), para
cobrir o boot, e o sleep hook (`/lib/systemd/system-sleep/aucc-lightbar`) passa
a chamar `--restore`.

As regras udev e as units existem em dois lugares: embutidas em `setup.rs`
(usadas por `aucc --install`) e em `install/70-avell-hid.rules` +
`install/install.sh`. Ambos precisam ser atualizados juntos.

### 5. Risco conhecido: corrida com o EC

Não está determinado se o EC apaga o LED no instante do evento de energia ou
alguns milissegundos depois. No segundo caso, o restore disparado pelo udev
seria sobrescrito. A mitigação será decidida por medição (ciclo de
conectar/desconectar cronometrado), entre: aplicar direto, `ExecStartPre` com
sleep curto, ou reaplicar duas vezes com intervalo. Não adotar mitigação sem o
dado.

## Testes

Testes unitários de round-trip e parsing de `KeyboardConfig`, espelhando os que
já existem para `LightbarConfig` (arquivo inexistente, vazio, só comentários,
config parcial, linhas inválidas, cada modo).

Verificação manual, com evidência registrada:

1. Configurar uma cor, desligar o cabo → o teclado mantém a cor.
2. Reconectar o cabo → mantém a cor.
3. Boot na bateria → o teclado acende com a configuração salva.
4. Suspend/resume em cada estado de energia → mantém a configuração.
5. `aucc --off` seguido de troca de energia → o teclado permanece apagado.

## Fora de escopo

- Alterar comportamento do EC/BIOS.
- Perfis de energia, telemetria, TDP.
- Refatoração não relacionada da TUI.
