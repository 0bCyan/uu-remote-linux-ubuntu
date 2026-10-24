# Ubuntu 二开说明 / Ubuntu fork notes

这是 [0bCyan/uu-remote-linux-ubuntu](https://github.com/0bCyan/uu-remote-linux-ubuntu)
在 [VallierDev/uu-remote-linux](https://github.com/VallierDev/uu-remote-linux)
**v0.2.29 / c041160d6dbbb48a4d7f2c5ed65cb39c6d6c34c5** 上的二开。
原项目核心实现与其他平台支持属于上游，完整署名见 [NOTICE.md](../NOTICE.md)。

## R1–R7 改动（2026-10-24）

| 快照 | 二开内容 |
|---|---|
| R1 | Intel VAAPI 进程内采集/编码的稳定性优化，保留 H.264 帧间预测链，减少闪烁与静态区域重复工作 |
| R2 | DRI3 局部损伤拷贝与 NV12 转换缓存、BT.709 色彩/范围、VAAPI QP 随画质设置、初版视频发送节奏；X11 边缘压力支持 Dock/热角 |
| R3 | GTK 主窗口的玻璃风格、卡片、滚动、DPI 字体与展开设置；保留账号及协助流程 |
| R4 | 保留控制端 Backspace/方向键等重复 key-down，修复输入框长按不能连续删除；修饰键维持按住状态 |
| R5 | 原视频与 NACK 重传共用限速；取消大帧限速时长截断；每连接反馈码率预算；原生 VAAPI 动态调码率与 VBV；1100 字节视频载荷分片 |
| R6 | 原生采集按开始时间调度，避免超时后额外睡眠；反馈中重复 NACK 去重；健康且受限速阻塞的链路恢复预算；可选全范围 BT.601 色彩兼容模式 |
| R7 | 合并上游 v0.2.30（QP 51、工具连接启动桌面）；转为桌面的工具连接保留原包/重传统一限速及适配 |

可用 `git diff c041160d6dbbb48a4d7f2c5ed65cb39c6d6c34c5..ubuntu` 查看与上游基线的完整差异。
后续上游合并及二开变更以 Git 历史为准。Cargo 版本号随上游更新为 `0.2.30`，不表示本二开由上游发布；
本轮版本标识为 **Ubuntu R7**。

## R7 上游合并与验证（2026-10-24）

合入 [v0.2.30 / 648eeca03d](https://github.com/VallierDev/uu-remote-linux/commit/648eeca03d0c7fc1fec67c08745fec060db10252)
及其前两个提交，保留真实上游历史。`vaapi_engine.c` 的冲突保留本二开的动态码率、首次帧 QP 初始化、
按画质选择的 min QP，以及上游 max QP 51；不把每次码率更新改回重置 QP。

- 本地完整测试 **270 passed, 0 failed, 8 ignored**，发布构建通过。
- 上游真实 SCTP 回归验证 `capture_desktop` 的重复请求、桌面启动/停止/再启动及工具连接响应。
- 新增真实 SRTP 回归：从工具连接协商 H.264，两轮启动/停止桌面，生成 64 KB 视频在 1 Mbps 预算下
  每轮发送至少 350 ms，RTP 序号连续，工具通道仍响应。限速器在所有 peer 建立时安装，仅拦截视频流，
  因此转为桌面的连接也沿用 R5/R6 原包/重传共享预算。
- Intel iHD、3120×2080、180 帧、384×384 / 768×768 生成变化区域、8→2→6 Mbps 动态预算：
  R6 与 R7 本轮编码输出一致。384 区域的 2 Mbps 阶段约 2.31 Mbps；768 极端噪声约 9.00 Mbps，仍会超预算。
  **本机没有测得 max QP 51 的额外码率收益**，不可套用上游 AMD 测量；真实流量仍受传输限速约束。
- 同一 3120×2080 / 60 Hz 生成动画、300 帧含启动：本轮 R6 42.17 fps、R7 42.74 fps，
  表现接近；这是本地 Xvfb 上传采集/编码对照，非手机实收帧率。
- 上传与 GPU 离屏 DMA 各 180 帧，检查完整解码、动态更新、缓存和 IDR 位置通过。
- 两条路径的 BT.709 和 `bt601-full` 64 色块/灰阶回归通过，R6 色彩转换和元数据保留。
- 上游限制保留：工具连接启动桌面使用已协商 H.264、无音频；Portal 不支持此路径。
  本地虚拟链路测试不代替官方 Mac 客户端黑屏问题的实机验收；本轮 R7 手机体验和长期稳定性待复验。

## R6 验证与色彩兼容（2026-10-24）

- 本地完整测试 **268 passed, 0 failed, 8 ignored**，发布构建通过。
- 同机、同一生成动画、隔离 Xvfb、3120×2080 / 60 Hz / VAAPI 上传、300 帧：
  R5 耗时 12.505 秒（含启动约 23.99 fps），R6 耗时 6.942 秒（约 43.22 fps）。
  这是本地编码/采集对照，**不等于手机实收帧率**；实际桌面与移动网络的长期统计尚未完成。
- 新增的时序回归覆盖快速、慢速编码及传输阻塞；不追赶过时采集时隙，保留静态保活策略。
- NACK 统计用固定 4096 槽、2 秒窗口去重，避免同一个缺包被重复请求而夸大降速信号。
  每次真实重传仍经过原有限速器；不以去重数声称最终丢包率下降。
- 六个健康窗口的原恢复条件之外，累计限速等待占窗口至少 10% 也视为有负载；
  无流量、无等待的静态画面仍不向上探测。原有反馈降速和控制端码率上限保留。
- 新增 `UU_VAAPI_COLOR=bt601-full`：VPP 输出全范围 BT.601，SPS 同步声明 full range / SMPTE170M 矩阵；
  RGB 原色和传递标记保留 BT.709。默认 `bt709` 保留原有限范围 BT.709。仅影响原生 VAAPI H.264。
- 此兼容模式来自公开 [WebRTC RTCMTLNV12Renderer](https://github.com/webrtc-mirror/webrtc/blob/main/sdk/objc/components/renderer/metal/RTCMTLNV12Renderer.mm)
  固定全范围 BT.601 系数这一线索；**不能据此确定 UU iOS 使用相同渲染器**。
- 64 个生成灰阶/混合色/色块、3120×2080，上传与 GPU 离屏 DMA 两条路径结果一致：
  标准解码最大通道误差 BT.709 为 4/255、兼容模式为 3/255；
  按上述固定渲染器系数模拟时，平均误差由 10.49 降至 0.92，最大由 35.57 降至 3.83。
  模拟结果本身不代表实际显示验收，也不包括 ICC、HDR 或系统显示滤镜。
- 本机启用兼容模式后，用户重连 iOS 控制端确认“颜色正常，明显更流畅”。这是本次配置的主观实机反馈，
  不代表所有 iPhone/iPad 版本、显示模式或网络均已验证；未测得手机端实际 FPS、长期稳定性或最终丢包率。
  删除色彩覆盖或设为 `bt709` 后重启服务即可恢复默认颜色模式。

## R5 已验证范围

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
| `UU_VAAPI_COLOR=bt601-full` | R6：原生 VAAPI H.264 全范围 BT.601 兼容模式；默认 `bt709`，本次 iOS 用户确认改善 |
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

R6 adds capture-start scheduling (generated Xvfb probe: ~24 to ~43 fps at 3120×2080),
bounded duplicate-NACK statistics and recovery under healthy pacing pressure. These are
local measurements, not delivered phone FPS or final packet-loss measurements. A selectable
native H.264 `bt601-full` mode has matched VPP/SPS signaling and passes generated upload/DMA
color checks. It targets fixed full-range BT.601 renderers like the public WebRTC Metal shader;
The user reconnected with their iOS viewer and confirmed normal color and noticeably smoother motion.
This is subjective acceptance for this configuration, not measured phone FPS, long-term stability
or final packet loss across devices/networks. Default color remains BT.709 limited.

R7 merges upstream v0.2.30 and retains R6 behavior. Promotion-capable tool peers install the
video-only pacing interceptor before negotiation; a real SRTP integration test verifies paced
video, sequence continuity and usable tool channels across two role changes. 270 local tests
pass. Intel generated-motion rate comparisons found no additional reduction from max QP 51;
extreme noise can still exceed the encoder target and remains transport-paced. Generated
upload/DMA color, cache, IDR and live-rate checks pass. Promoted tool peers have H.264 video
without audio; Portal promotion and official Mac end-to-end acceptance remain unverified.
