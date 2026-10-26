# 来源、署名与二次开发 / Attribution and derivative work

## 原始项目 / Upstream

- 项目：**uu-remote-linux**，由 **VallierDev 与上游贡献者**开发。
- 仓库：<https://github.com/VallierDev/uu-remote-linux>
- 本二开基线：**v0.2.29**，提交 `c041160d6dbbb48a4d7f2c5ed65cb39c6d6c34c5`。
- 已合入上游：**v0.2.32**，提交 `ab5d0a656bee2736886321529b72ea6e0bb5130e`；原始二开基线保留不变。
- 上游贡献记录：<https://github.com/VallierDev/uu-remote-linux/graphs/contributors>

This repository is derived from **VallierDev/uu-remote-linux**, not an independent
implementation of its core functionality. The upstream project and its contributors
are credited for the original protocol, host/viewer, capture, input, services and tools.
The upstream Git history and existing copyright/license notices are retained.

## 本二开 / This fork

- 仓库：<https://github.com/0bCyan/uu-remote-linux-ubuntu>
- 维护者：**0bCyan**。
- 二开快照：**Ubuntu R8**；修改日期：**2026-10-26**。
- 范围：Ubuntu X11 / Intel VAAPI 的采集、色彩、视频发送、网络反馈适配，
  GNOME Dock 边缘输入、长按重复及 GTK 主窗口样式。详见 [变更记录](docs/UBUNTU_FORK.md)。
- 二开维护与测试借助 Codex 完成；这不改变上游署名或许可证。

The Ubuntu-specific changes are maintained by **0bCyan**, with implementation and
validation assisted by Codex. Changed implementation files carry a dated fork notice;
Git diffs against the baseline identify the changes. Inherited documentation and test
claims remain upstream work unless the fork validation notes state otherwise.

## 许可证 / Licensing

The upstream [LICENSE](LICENSE) is preserved verbatim. This fork retains upstream GPL
licensing; its Cargo SPDX identifier remains **GPL-3.0-or-later**. No additional
restrictions are imposed on the fork's modifications. Source and applicable license
notices must accompany redistribution as required by their respective licenses.

Third-party components retain their own terms, including:

- `vendor/webrtc`, `vendor/webrtc-dtls`, `vendor/webrtc-srtp`: their bundled MIT and
  Apache-2.0 license files.
- `vendor/ffnvcodec`: NVIDIA/FFmpeg contributor notices and terms in the bundled headers.
- Cargo dependencies and system libraries such as FFmpeg, GTK, libva and X11: their
  respective licenses. Their authorship is not transferred to this fork.

## 非官方声明 / Non-affiliation

本项目与网易、Apple 及上游维护者无隶属关系，未获其官方授权或背书。
「网易」「UU 远程」等名称和商标属于对应权利人。本仓库不分发网易 Windows
程序、账号凭证或 Apple 设计素材；玻璃风格界面是本项目的 GTK 实现。

This fork is not affiliated with or endorsed by NetEase, Apple or the upstream
maintainer. Their names and marks belong to their respective owners. No NetEase
Windows binaries, account credentials or Apple design assets are distributed.
