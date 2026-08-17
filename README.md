<p align="center">
  <img src="packaging/icons/io.github.Pianisuto.LionClip.svg" alt="Ícone do LionClip" width="112" height="112">
</p>

<h1 align="center">LionClip</h1>

<p align="center">
  Histórico da área de transferência rápido, nativo e privado para Linux.
</p>

<p align="center">
  <a href="https://github.com/Pianisuto/LionClip/releases"><img alt="GitHub Release" src="https://img.shields.io/github/v/release/Pianisuto/LionClip?display_name=tag&sort=semver"></a>
  <a href="https://github.com/Pianisuto/LionClip/actions/workflows/rust.yml"><img alt="CI" src="https://github.com/Pianisuto/LionClip/actions/workflows/rust.yml/badge.svg"></a>
  <img alt="Linux GNOME/Zorin" src="https://img.shields.io/badge/plataforma-Linux%20%C2%B7%20GNOME%2FZorin-6f5af0">
  <img alt="Dados locais" src="https://img.shields.io/badge/dados-100%25%20locais-35b779">
</p>

O **LionClip** traz uma experiência de histórico da área de transferência no estilo `Win+V` para Linux, com foco inicial em **Zorin OS / GNOME**. Pressione `Super+V`, encontre o que copiou e continue trabalhando sem abrir um aplicativo tradicional.

Tudo fica local no computador: textos, imagens, preferências e histórico persistente. Sem conta, nuvem, telemetria ou serviço remoto.

## Recursos

- histórico persistente de textos em SQLite;
- screenshots e imagens PNG/JPEG com thumbnails;
- busca instantânea enquanto digita;
- navegação completa por teclado e mouse;
- fixar, excluir e limpar itens do histórico;
- deduplicação e limites configuráveis de retenção;
- popup compacto próximo ao ponteiro no GNOME/X11;
- `Super+V` configurável por helper próprio, sem extensão do GNOME Shell;
- inicialização automática com o sistema;
- preferências nativas em Libadwaita;
- pausa da captura e opção para ignorar novas imagens;
- **auto-paste opcional no X11**, desligado por padrão;
- processo residente único, sem polling agressivo do clipboard.

## Instalação

Baixe a versão mais recente na página de [Releases](https://github.com/Pianisuto/LionClip/releases).

O pacote atual é para **Ubuntu 24.04 / Zorin OS baseado em Noble, amd64**:

```bash
sudo apt install ./lionclip_0.1.0_amd64.deb
```

O `.deb` instala o aplicativo, ícone, launcher, autostart, schema de preferências e o helper do atalho.

### Primeiro uso

O LionClip inicia automaticamente nos próximos logins. Para iniciar imediatamente:

```bash
setsid lionclip >/dev/null 2>&1 &
```

Configure `Super+V`:

```bash
lionclip-shortcut install
```

Se o GNOME já estiver usando `Super+V` para notificações, o helper não sobrescreve nada sem permissão. Para assumir o atalho explicitamente:

```bash
lionclip-shortcut install --take-over
```

Verifique ou remova a configuração a qualquer momento:

```bash
lionclip-shortcut status
lionclip-shortcut remove
```

## Uso

Pressione `Super+V` para abrir o histórico e novamente para fechar.

- **buscar:** digite normalmente;
- **navegar:** `↑` / `↓`;
- **restaurar:** `Enter` ou clique no item;
- **fechar:** `Escape` ou clique fora;
- **fixar:** `Ctrl+P` ou botão de pin;
- **excluir:** `Delete`;
- **imagens:** screenshots e imagens copiadas aparecem como thumbnails e são restauradas no formato original;
- **preferências:** menu `⋮` → **Preferences**.

Por padrão, selecionar um item apenas o restaura para o clipboard. Depois use `Ctrl+V` normalmente. No X11, a preferência **Automatically paste selected items** pode fazer essa colagem automaticamente; ela permanece desativada por padrão por segurança. Veja [`SECURITY.md`](SECURITY.md).

### Linha de comando

```bash
lionclip           # inicia a instância residente sem abrir o popup
lionclip show      # mostra o popup
lionclip hide      # esconde o popup
lionclip toggle    # alterna entre mostrar e esconder
lionclip settings  # abre/foca Preferências
```

Todas as chamadas conversam com a mesma instância residente.

## Preferências

As configurações persistem via GSettings e são aplicadas sem reiniciar o aplicativo:

- limite do histórico: 100 / 250 / 500 / 1000 itens não fixados;
- salvar ou ignorar novas imagens copiadas;
- pausar e retomar a captura do histórico;
- colar automaticamente ao selecionar — **X11 apenas**;
- iniciar com o sistema;
- limpar todo o histórico, inclusive itens fixados e imagens armazenadas.

Itens fixados não são removidos pelo limite normal de retenção.

## Privacidade

O LionClip não envia conteúdo da área de transferência para servidores externos e não possui telemetria.

O histórico fica em `$XDG_DATA_HOME/lionclip`, normalmente:

```text
~/.local/share/lionclip
```

A remoção do pacote preserva esses dados de propósito. Para apagá-los manualmente:

```bash
rm -rf ~/.local/share/lionclip
```

## Atualização e remoção

Para atualizar, instale o `.deb` novo por cima do atual:

```bash
sudo apt install ./lionclip_<versao>_amd64.deb
```

Uma instância já em execução continua usando o binário antigo até reiniciar ou fazer logout/login.

Antes de desinstalar, remova o atalho do GNOME enquanto o helper ainda existe:

```bash
lionclip-shortcut remove
sudo apt remove lionclip
```

Use `sudo apt purge lionclip` se também quiser remover o bookkeeping do pacote. O histórico pessoal continua preservado até você apagá-lo explicitamente.

## Plataforma

O alvo principal validado é:

- Zorin OS baseado em Ubuntu 24.04 (`noble`);
- GNOME;
- sessão X11;
- arquitetura amd64.

No X11, o popup pode ser posicionado próximo ao ponteiro e o auto-paste opcional possui backend próprio. No Wayland nativo, o compositor controla a posição do popup e o auto-paste fica indisponível; restaurar o clipboard continua funcionando normalmente. XWayland permanece experimental.

## Desenvolvimento

Dependências nativas em Ubuntu/Zorin Noble:

```bash
sudo apt install build-essential libadwaita-1-dev libgtk-4-dev libx11-dev pkg-config
```

Com Rust stable:

```bash
git clone https://github.com/Pianisuto/LionClip.git
cd LionClip
cargo build
cargo test --all-features
cargo run -- show
```

Verificações antes de enviar mudanças:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release --locked
```

Empacotamento:

```bash
packaging/deb/build.sh
```

O pacote é gerado em `target/deb/lionclip_<version>_amd64.deb`.

### Publicação de versões

O workflow [`release.yml`](.github/workflows/release.yml) é disparado por tags `v*`. Ele verifica se a tag corresponde à versão do `Cargo.toml`, roda os checks, gera o `.deb`, cria `SHA256SUMS.txt` e publica tudo na mesma GitHub Release.

Exemplo para uma nova versão:

```bash
# atualize version em Cargo.toml/Cargo.lock antes
git tag v0.1.0
git push origin v0.1.0
```

## Tecnologias

- **Rust** — aplicação e domínio;
- **GTK4 / gtk4-rs** — interface e clipboard;
- **Libadwaita** — visual nativo e Preferências;
- **GLib / GIO** — lifecycle e single-instance;
- **SQLite / rusqlite** — histórico local;
- **x11rb** — posicionamento e auto-paste isolados no X11.

A arquitetura, decisões de plataforma e roadmap ficam em [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) e [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Licença

Uma licença ainda não foi selecionada. Enquanto não houver um arquivo `LICENSE`, o fato de o repositório ser público não concede permissão para copiar, modificar ou redistribuir o código além do permitido pelos Termos de Serviço do GitHub.
