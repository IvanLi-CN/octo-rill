---
title: 产品说明
description: OctoRill 的页面结构、核心体验与数据边界。
---

# 产品说明

本文档整理自仓库内 `docs/product.md`，用于帮助协作者快速理解 OctoRill 的核心信息架构。

## 核心体验

OctoRill 的目标不是替代 GitHub，而是把 GitHub 上分散的更新整理成一个更适合阅读的工作台：

- Feed：以 Release 为主的信息流，支持无限滚动与中文翻译。
- 日报：汇总过去一天值得关注的 Release 更新。
- Inbox：快速回看 GitHub 通知，减少在原生通知页来回跳转。
- Admin：观察后台同步、翻译与任务调度状态。

## 页面结构

### Landing

Landing 页面承担登录入口和产品定位介绍：未登录用户先在这里完成 GitHub OAuth。

### Dashboard

Dashboard 是登录后的主工作区，包含：

- Header 与全局切换。
- Release Feed 主列。
- 右侧 Brief / Inbox / Release Detail 辅助区块。
- Footer 中的构建版本与元信息展示。

### Admin Panel

Admin Panel 聚焦用户管理：查看用户、搜索筛选、启停用户、管理员身份确认等。

### Admin Jobs

Admin Jobs 聚焦后台任务可观测性：实时任务、计划任务、LLM 调用、任务详情与日志入口。

## AI 翻译策略

- 默认把 Release 内容翻译成中文。
- 翻译与日报属于增强能力，不应阻塞主阅读流程。
- 翻译内容以“共享事实语义”处理，不因 Star 取消而抹掉已有历史上下文。

## 数据来源与同步

- OAuth 负责登录、读取 Feed / Notifications / Starred / Releases。
- PAT 只用于 Release 反馈等额外写操作。
- 本地 SQLite 负责缓存与派生数据，不直接替代 GitHub 作为事实源。

## 非目标

- 不在站内直接取代 GitHub 的完整交互能力。
- 不把所有 GitHub 通知与发布管理行为都闭环在本项目内。
