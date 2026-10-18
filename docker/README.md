# 群晖 / NAS 容器部署（无桌面）

**简体中文** | [English](#english)

在没有桌面的 NAS（群晖 DSM 等，x86_64）上，用容器运行本项目主机，供官方 UU 远程客户端使用：

- **文件传输**：收发 `/data` 里的文件（把你的共享文件夹挂到 `/data`）。
- **终端**：进入容器内的 bash，当前目录为 `/data`。
- **端口映射**：在外网经 UU 访问家里局域网的服务（NAS 管理页、路由器、其他电脑……）。

不提供远程桌面（NAS 没有图形界面）。容器里运行的是 GitHub 发布包中的同一个 `uuremote-server`。

## 安装

在 NAS 上（SSH 登录后用 root 或 `sudo`），进入本目录：

```sh
mkdir -p config data && chmod 700 config
docker compose build          # 或者直接使用 ghcr.io/vallierdev/uu-remote-linux 镜像
```

**首次登录**（只需一次）：在你自己的终端里运行，用手机 UU 远程扫描终端里显示的二维码并确认。
二维码等同于登录凭据，不要截图或发给别人。

```sh
docker compose run --rm uu-remote login --no-gui
```

登录信息保存在 `config/config.json`（权限 0600），请妥善保管 `config` 目录。然后启动：

```sh
docker compose up -d
docker compose logs -f        # 看到 availability=Some("ready") 即可在客户端设备列表里找到它
```

**群晖 Container Manager**：也可以在「项目」里新建项目，选本目录的 `compose.yaml`；首次登录仍需在 SSH 里执行上面的 `login` 命令。

## 说明

- `network_mode: host` 让 WebRTC 走直连；改用默认桥接网络也能用，但更常走中转，延迟更高。
- 把 `compose.yaml` 里的 `./data` 改成要开放的共享文件夹，例如 `/volume1/homes/你的用户名`。只挂需要远程访问的目录。
- 终端操作的是容器而不是 NAS 系统本身；需要操作 NAS 本机时请用 DSM 自带的 SSH。
- 升级：重新 `docker compose build --pull`（或拉取新镜像）后 `docker compose up -d`；`config` 保留即可免再登录。

---

## English

Run this project's host in a container on a desktop-less NAS (Synology DSM, x86_64) for the official UU Remote clients:
file transfer (`/data`), terminal (bash inside the container) and port mapping into your LAN. No remote desktop.

```sh
mkdir -p config data && chmod 700 config
docker compose build
docker compose run --rm uu-remote login --no-gui   # scan the terminal QR with the UU mobile app (the QR is a credential)
docker compose up -d
docker compose logs -f                              # wait for availability=Some("ready")
```

Credentials are kept in `config/config.json`. Mount only the folders you want to reach as `/data`.
Host networking gives WebRTC direct paths; bridge networking works but relays more often.

**Unofficial project, not affiliated with NetEase.**
