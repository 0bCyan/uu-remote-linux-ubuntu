# Ubuntu 二开说明 / Ubuntu fork notes

这是 [0bCyan/uu-remote-linux-ubuntu](https://github.com/0bCyan/uu-remote-linux-ubuntu)
在 [VallierDev/uu-remote-linux](https://github.com/VallierDev/uu-remote-linux)
**v0.2.29 / c041160d6dbbb48a4d7f2c5ed65cb39c6d6c34c5** 上的二开。
原项目核心实现与其他平台支持属于上游，完整署名见 [NOTICE.md](../NOTICE.md)。

## R1–R5 改动（2026-10-24）

| 快照 | 二开内容 |
|---|---|
| R1 | Intel VAAPI 进程内采集/编码的稳定性优化，保留 H.264 帧间预测链，减少闪烁与静态区域重复工作 |
| R2 | DRI3 局部损伤拷贝与 NV12 转换缓存、BT.709 色彩/范围、VAAPI QP 随画质设置、初版视频发送节奏；X11 边缘压力支持 Dock/热角 |
| R3 | GTK 主窗口的玻璃风格、卡片、滚动、DPI 字体与展开设置；保留账号及协助流程 |
| R4 | 保留控制端 Backspace/方向键等重复 key-down，修复输入框长按不能连续删除；修饰键维持按住状态 |
| R5 | 原视频与 NACK 重传共用限速；取消大帧限速时长截断；每连接反馈码率预算；原生 VAAPI 动态调码率与 VBV；1100 字节视频载荷分片 |

可用 `git diff c041160d6dbbb48a4d7f2c5ed65cb39c6d6c34c5..ubuntu` 查看与上游基线的完整差异。
后续上游合并及二开变更以 Git 历史为准。Cargo 版本号暂保留 `0.2.29`，不表示本二开由上游发布；
本轮版本标识为 **Ubuntu R5**。

## 已验证范围

主要环境：Ubuntu 24.04、GNOME X11、Intel Meteor Lake / iHD、VAAPI H.264，桌面 3120×2080。
用户在手机控制端确认 R5 体验有所改善；这不是长期可用性或任意网络性能保证。

- 完整本地测试：`265 passed, 0 failed, 8 ignored`，特性为
  `wayland,portal,engine-plugins,inproc-av`，目标 `uuremote-server`。
- 完整发布构建与 GTK 链接检查通过。测试机缺少满足版本要求的 libei 开发包时，构建跳过
  可选 Portal 插件；本轮没有重新验证 Wayland/Portal 的端到端性能。
- 实际 SRTP 经虚拟链路发送 300 KB 画面、274 包：

  | 链路 / 队列 | 不限速丢包 | R5 限速丢包 |
  |---|---:|---:|
  | 8 Mbps / 12 KB | 123 / 274 | 0 / 274 |
  | 3 Mbps / 6 KB | 232 / 274 | 0 / 274 |

  这是可重复的模拟对照；调度会影响未限速计数，不能视为真实手机丢包率。
- 实际 NACK responder 测试确认重传经过相同预算；覆盖反馈降速、缓慢恢复、空闲与稀疏报告、媒体流隔离。
- 本机 Intel VAAPI 上传与离屏 DMA 各 180 帧：动态码率变化后完整解码、局部变化、缓存帧与 IDR 位置正常。
- 3120×2080、384×384 持续变化区域：8 Mbps 阶段约 6.58 Mbps；降为 2 Mbps 后约 1.98 Mbps；
  恢复到 6 Mbps 后约 4.30 Mbps，完整解码无错。
- BT.709 限幅元数据检查及色块最大通道误差 2/255；不代表 ICC/显示器校色精度。
- 输入重复及边缘屏障探测使用隔离 Xvfb；UI 预览使用生成内容及不可扫描的预览二维码。

## R5 运行参数与边界

视频原包与重传共用约 3.6 KB 突发的令牌桶，线上预算为编码预算的 1.15 倍。
初始预算为 6 Mbps 或控制端选择的更低上限；有效反馈窗口至少 2 秒、80 个已发送包。
重传压力/有效 RR 丢包增高时降低预算，连续六个有足够负载的健康窗口后小幅增加。
静态画面不向上探测，稀疏接收报告不作为有效丢包样本；重传请求比例不是最终未恢复丢包率。

原生 VAAPI 在编码线程更新码率与同比例 VBV，不重启预测链。
CPU/NVENC/外部 FFmpeg 目前只受发送预算限制，尚无同样的实时编码码率调整。
极端大面积随机噪声可能超过编码目标；此时发送限速以减少帧率来约束流量。
拥塞时清晰度与帧率需要权衡，不保证任意分辨率/内容下同时达到最高画质和帧率。

| 环境变量 | 含义 |
|---|---|
| `UU_VIDEO_PACING=0` | 关闭原包与重传的发送节奏控制，默认开启 |
| `UU_VIDEO_ADAPTIVE=0` | 关闭反馈适配，预算使用控制端选择的码率上限，默认开启 |
| `UU_VAAPI_BUFFER_MS=250` | 测试机使用的 250 ms VBV；缓冲较小时高分辨率关键帧可能变糊 |
| `UU_X11_ABSOLUTE_WARP=1` | 恢复旧的 X11 绝对定位路径，默认使用支持边缘压力的路径 |

Intel Ubuntu 通常需要 `intel-media-va-driver`（部分系统使用 non-free 变体），
并确保桌面用户可访问 `/dev/dri/renderD128`。不要为了运行程序随意开放设备的全局写权限。

构建命令及依赖见 [README](../README.md)。推荐本二开 X11 构建使用 `--features inproc-av`，
选择 `set-encoder vaapi` 与 `set-codec h264`。跨桌面插件构建另需对应 Wayland/Portal 开发依赖。

已有用户服务可在 `systemctl --user edit uuremote-native.service` 中添加：

```ini
[Service]
Environment=UU_VAAPI_BUFFER_MS=250
```

然后执行 `systemctl --user daemon-reload` 和 `systemctl --user restart uuremote-native.service`。
修改或重启会断开当前远程连接。更新运行中的二进制时，复制至临时文件再原子替换；
不要直接覆盖正在执行的文件。首次服务安装、登录与会话环境配置见主 README。

## 复现与贡献

- [视频、色彩、DMA、动态码率探测](../contrib/video-tests/README.md)
- [按键重复探测](../contrib/input-tests/README.md)
- [离线 UI 预览](../contrib/ui-preview/README.md)

不要提交账号配置、QR 登录 URL、设备/中继地址、令牌、键盘输入或真实剪贴板内容。
Issue 请优先提供 `uuremote-server doctor` 的脱敏状态及数值统计；原始日志应先审阅、脱敏。

## English summary

This unofficial fork credits VallierDev and all upstream contributors, retains GPL
licensing and upstream history, and focuses on Ubuntu X11 / Intel VAAPI. R1–R5 cover
capture caching, BT.709 color, shared video/repair pacing, feedback-driven budgets,
Dock edge pressure, repeated-key fixes and a GTK glass-inspired UI.

Local validation includes 265 passing tests (8 ignored), real Intel upload/offscreen
DMA encoding and simulated bounded-link SRTP traffic. Virtual-link zero-drop results
are not a claim of zero loss on real mobile networks. Live encoder rate adaptation
currently applies to native VAAPI only. Extreme complexity or congestion can reduce
quality and frame rate. Wayland/Portal performance was not revalidated in this fork.
Build with `--features inproc-av` for the tested X11 path; see the main README for
setup and the linked probe documentation for reproducible checks.
