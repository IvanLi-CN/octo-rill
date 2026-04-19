## LinuxDO Connect deployment

用于自托管 OctoRill 实例启用 LinuxDO 账号绑定。

### Scope

- 这是可选能力；不影响 GitHub OAuth 登录、PAT 设置与日报功能。
- 未配置 LinuxDO OAuth 时，OctoRill 仍可正常运行，只是 `/settings` 中的 LinuxDO 绑定区块保持禁用。

### Required environment variables

LinuxDO 绑定只接受以下三项配置同时出现：

```bash
LINUXDO_CLIENT_ID=<linuxdo-client-id>
LINUXDO_CLIENT_SECRET=<linuxdo-client-secret>
LINUXDO_OAUTH_REDIRECT_URL=https://octorill.example.com/auth/linuxdo/callback
```

规则：

- 三项都为空：LinuxDO 绑定视为未启用。
- 三项都填写：LinuxDO 绑定启用。
- 只填写其中一部分：OctoRill 启动失败。

### Deployment steps

1. **确定对外访问地址**
   - 先确定你的 OctoRill 公网地址，例如 `https://octorill.example.com`。
   - LinuxDO callback URL 固定写成：`<public-origin>/auth/linuxdo/callback`。
   - 这个地址必须是 LinuxDO Connect 能访问到的地址，而不是容器内地址、内网地址或 `127.0.0.1`。

2. **配置反向代理 / 网关**
   - 确保 `https://octorill.example.com/auth/linuxdo/callback` 会转发到 OctoRill 后端。
   - 如果 OctoRill 部署在反向代理后面，域名、协议与端口都以用户最终访问的公开地址为准。
   - 如果前端与后端共用同一域名，至少要保证 `/auth/linuxdo/callback` 这条路径不会被静态站点吞掉。

   一个最小 Nginx 代理示意：

```nginx
location / {
    proxy_pass http://octorill-web;
}

location /auth/linuxdo/callback {
    proxy_pass http://octorill-backend;
}
```

   如果你是统一把站点入口全部代理到后端，再由后端服务静态资源，也可以只保留一个上游；关键是 callback 要命中 OctoRill 后端。

3. **在 LinuxDO Connect 后台登记应用**
   - 创建或编辑 LinuxDO Connect 应用。
   - 把 callback URL 填成上面确定的完整地址，例如：

```text
https://octorill.example.com/auth/linuxdo/callback
```

   - 保存后记录发放的 `client_id` 与 `client_secret`。
   - LinuxDO 官方接入说明见：[Linux DO Connect](https://wiki.linux.do/Community/LinuxDoConnect)

4. **写入 OctoRill 环境变量**
   - 在 OctoRill 运行环境中同时写入三项变量：

```bash
LINUXDO_CLIENT_ID=<linuxdo-client-id>
LINUXDO_CLIENT_SECRET=<linuxdo-client-secret>
LINUXDO_OAUTH_REDIRECT_URL=https://octorill.example.com/auth/linuxdo/callback
```

   - `LINUXDO_OAUTH_REDIRECT_URL` 必须与 LinuxDO Connect 后台登记值完全一致。

   如果你使用 `docker compose`，可按下例传入：

```yaml
services:
  octorill:
    image: <your-octorill-image>
    env_file:
      - .env.local
    environment:
      LINUXDO_CLIENT_ID: ${LINUXDO_CLIENT_ID}
      LINUXDO_CLIENT_SECRET: ${LINUXDO_CLIENT_SECRET}
      LINUXDO_OAUTH_REDIRECT_URL: ${LINUXDO_OAUTH_REDIRECT_URL}
```

   如果你使用 `systemd`，可在环境文件中写入：

```ini
LINUXDO_CLIENT_ID=your-linuxdo-client-id
LINUXDO_CLIENT_SECRET=your-linuxdo-client-secret
LINUXDO_OAUTH_REDIRECT_URL=https://octorill.example.com/auth/linuxdo/callback
```

5. **重启服务**
   - 重启 OctoRill 后端进程，使配置生效。

6. **验证配置**
   - 用已登录 OctoRill 的浏览器访问 `/settings?section=linuxdo`。
   - 正常结果应为：
     - 页面不再显示“暂未启用 LinuxDO 绑定”
     - 点击“连接 LinuxDO”后会跳转到 LinuxDO Connect 授权页
     - 授权完成后会回到 `/settings?section=linuxdo`，并展示绑定后的 LinuxDO 快照

### Disable / rollback

如果不需要 LinuxDO 绑定，请同时移除以下三项：

```bash
LINUXDO_CLIENT_ID
LINUXDO_CLIENT_SECRET
LINUXDO_OAUTH_REDIRECT_URL
```

只删除其中一部分会让服务因为配置不完整而拒绝启动。

### What OctoRill stores

OctoRill 只保存 LinuxDO 绑定快照：

- `linuxdo_user_id`
- `username`
- `name`
- `avatar_url`
- `trust_level`
- `active`
- `silenced`
- `linked_at`
- `updated_at`

OctoRill 不持久化以下内容：

- LinuxDO access token
- LinuxDO refresh token
- LinuxDO `api_key`

### Common mistakes

- `LINUXDO_OAUTH_REDIRECT_URL` 与 LinuxDO Connect 后台登记值不一致
- 把 callback 配成内网地址、容器内地址、`127.0.0.1` 或错误端口
- 反向代理没有转发 `/auth/linuxdo/callback`
- 只配置了 `LINUXDO_CLIENT_ID` / `LINUXDO_CLIENT_SECRET` 中的一部分
- 把 `LINUXDO_CLIENT_SECRET` 暴露到前端代码、静态文件或仓库提交中

### Operational check

如果你想快速判断 LinuxDO OAuth 是否已经启用：

- 打开设置页的 LinuxDO 区块
- 若按钮可点击并能跳转授权页，说明服务端配置已启用
- 若区块显示“暂未启用 LinuxDO 绑定”，说明服务端仍处于未配置状态

一个最小验收清单：

- `/settings?section=linuxdo` 不再显示“暂未启用 LinuxDO 绑定”
- 点击“连接 LinuxDO”后浏览器会跳转到 LinuxDO Connect 授权页
- 授权结束后回跳 `/settings?section=linuxdo`
- 页面能看到 LinuxDO 头像、用户名与绑定状态
