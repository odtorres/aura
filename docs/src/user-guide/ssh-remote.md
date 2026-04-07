# SSH Remote Editing

Edit files on remote machines via SSH directly from AURA.

## Usage

```
:ssh user@host:/path/to/file
:ssh myserver:/etc/nginx/nginx.conf
:ssh deploy@prod.example.com:/app/config.yaml
```

## How It Works

1. AURA runs `ssh host cat path` to fetch the file content
2. The file opens in a new tab with full syntax highlighting and LSP
3. When you save (`:w`), AURA writes back via `ssh host tee path`
4. A local cache is stored at `/tmp/aura-ssh/<host>/` for the session

## Requirements

- `ssh` command available in your PATH
- SSH keys or agent configured for the target host
- Your `~/.ssh/config` is respected (aliases, ports, keys)

## Features

- Full editor features work: syntax highlighting, tree-sitter, LSP
- Save goes back to the remote host automatically
- Status bar shows the remote path
- No extra setup — uses your existing SSH configuration

## Port Override

For non-standard SSH ports, use your `~/.ssh/config`:

```
Host myserver
    HostName 192.168.1.100
    Port 2222
    User deploy
```

Then: `:ssh myserver:/path/to/file`
