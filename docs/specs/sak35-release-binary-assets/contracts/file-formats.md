# 文件格式（File formats）

## `octo-rill-linux-x86_64.tar.gz`（GitHub Release asset）

- 范围（Scope）: external
- 变更（Change）: New
- 编码（Encoding）: binary

### Schema（结构）

- `octo-rill-linux-x86_64/`
- `octo-rill-linux-x86_64/octo-rill`
- `octo-rill-linux-x86_64/.env.example`
- `octo-rill-linux-x86_64/web/dist/**`

### Examples（示例）

- `tar -xzf octo-rill-linux-x86_64.tar.gz`
- `./octo-rill-linux-x86_64/octo-rill`

### 兼容性与迁移（Compatibility / migration）

- 顶层目录名固定，便于 README、发布页与后续自动化脚本直接引用。
- 后续如增加多平台包，需新增独立资产名，不得静默修改本资产的目录结构。
