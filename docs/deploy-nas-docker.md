# 群晖 / NAS 部署指南（Docker，无桌面）

**简体中文** | [English](#english)

镜像：[`ghcr.io/vallierdev/uu-remote-linux`](https://github.com/VallierDev/uu-remote-linux/pkgs/container/uu-remote-linux)
（标签 `latest` 或具体版本如 `v0.2.8`）

把本项目主机跑在没有桌面的 NAS 上之后，就能在外面用官方 UU 远程客户端（手机 / Mac / Windows）：

| 功能 | 说明 |
|---|---|
| 文件传输 | 收发 NAS 上的文件（只开放你挂进容器的文件夹） |
| 终端 | 打开容器里的命令行（bash） |
| 端口映射 | 经 UU 访问家里局域网的服务：NAS 管理页、路由器、其他电脑的 SSH / 远程桌面等 |

**不提供远程桌面**（NAS 没有图形界面，点「远程桌面」会一直显示正在连接，属正常）。

## 准备

- x86_64 的 NAS（群晖 DSM 7.2 及以上已安装「Container Manager」，或任意装了 Docker 的 Linux NAS）。ARM 机型暂不支持。
- 能 SSH 登录 NAS（群晖：控制面板 → 终端机和 SNMP → 启用 SSH），并有管理员权限（`sudo`）。
- 手机上的 UU 远程 App，用于扫码登录。

## 第一步：建目录和配置

SSH 登录 NAS，新建一个目录（下面以 `/volume1/docker/uu-remote` 为例）：

```sh
sudo mkdir -p /volume1/docker/uu-remote/config
sudo chmod 700 /volume1/docker/uu-remote/config
cd /volume1/docker/uu-remote
```

新建 `compose.yaml`，内容如下。把 `/volume1/共享文件夹` 改成你想远程访问的文件夹（可以写多行挂多个）：

```yaml
services:
  uu-remote:
    image: ghcr.io/vallierdev/uu-remote-linux:latest
    container_name: uu-remote
    network_mode: host          # 让远程连接尽量走直连，速度更快
    restart: unless-stopped
    volumes:
      - ./config:/config        # 登录信息，请勿外传
      - /volume1/共享文件夹:/data  # 文件传输和终端看到的目录
```

## 第二步：扫码登录（只需一次）

```sh
sudo docker compose pull
sudo docker compose run --rm uu-remote login --no-gui
```

终端里会显示一个二维码：用手机 UU 远程扫码并确认。看到登录成功的提示后，按 `Ctrl+C` 退出（如果没有自动退出）。

> 二维码等同于你的登录凭据，**不要截图或发给别人**。登录信息保存在 `config/config.json`。
> 一定要用上面的 `docker compose run`（主机网络），这样设备在列表里显示的是 NAS 的名字；
> 若用普通 `docker run` 登录，设备名会是一串随机字符。

## 第三步：启动

```sh
sudo docker compose up -d
sudo docker compose logs -f
```

日志里出现 `availability=Some("ready")` 就说明上线了，按 `Ctrl+C` 退出日志查看（容器继续在后台运行）。
之后 NAS 开机会自动启动它。

**用群晖界面管理**：Container Manager → 项目 → 新增，路径选 `/volume1/docker/uu-remote`，会自动读取 `compose.yaml`。
首次登录仍需在 SSH 里执行第二步。

## 使用

打开官方 UU 远程客户端，在「我的设备」里找到你的 NAS（显示为 Windows 设备，这是正常的）：

- **文件传输**：浏览、上传、下载 `/data` 里的文件。
- **终端**：进入容器的命令行，当前目录是 `/data`。注意这是容器而不是 NAS 系统本身。
- **端口映射**：例如目标填 `127.0.0.1`、端口 `5000`，就能在本地浏览器打开 NAS 的 DSM 管理页；
  填局域网里其他设备的地址和端口（如 `192.168.1.10:22`）即可访问那台设备的服务。

## 更新

```sh
cd /volume1/docker/uu-remote
sudo docker compose pull && sudo docker compose up -d
```

`config` 目录保留即可，不用重新登录。

## 停用与移除

```sh
sudo docker compose down          # 停止并删除容器
```

再在官方客户端的设备列表里删除这台设备；如需彻底清除登录信息，删除 `config` 目录。

## 常见问题

- **设备名是一串随机字符**：登录时没有用主机网络。删除 `config/config.json`，按第二步用 `docker compose run` 重新登录。
- **一直不上线**：`sudo docker compose logs --tail 100` 查看日志；确认 NAS 能访问外网。
- **连接慢、经常卡**：确认 `network_mode: host` 没有被去掉。
- **想要远程桌面**：NAS 上不支持；请在有桌面的 Linux 电脑上安装主机端（见主 README）。

安全提示：只挂需要远程访问的文件夹；端口映射可以访问 NAS 所在局域网，请保管好 UU 账号。

---

## English

Image: [`ghcr.io/vallierdev/uu-remote-linux`](https://github.com/VallierDev/uu-remote-linux/pkgs/container/uu-remote-linux)

Runs this project's host headless on a desktop-less NAS (x86_64, e.g. Synology DSM 7.2+ with Container Manager) for the
official UU Remote clients: **file transfer** (folders you mount at `/data`), **terminal** (bash inside the container) and
**port mapping** into your LAN. No remote desktop.

```sh
sudo mkdir -p /volume1/docker/uu-remote/config && sudo chmod 700 /volume1/docker/uu-remote/config
cd /volume1/docker/uu-remote
# compose.yaml: image ghcr.io/vallierdev/uu-remote-linux:latest, network_mode: host,
#   volumes ./config:/config and /volume1/your-share:/data  (see the Chinese section above)
sudo docker compose pull
sudo docker compose run --rm uu-remote login --no-gui   # scan the QR with the UU mobile app; the QR is a credential
sudo docker compose up -d
sudo docker compose logs -f                             # wait for availability=Some("ready")
```

Update with `docker compose pull && docker compose up -d` (keep `config/`). Remove with `docker compose down`, then delete
the device in the official client.

**Unofficial project, not affiliated with NetEase.**
