# 命令行（CLI）

## GitHub Actions `Release` workflow_dispatch

- 范围（Scope）: internal
- 变更（Change）: Modify

### 用法（Usage）

```text
GitHub Actions -> Release -> Run workflow
```

### 参数（Args / options）

- `release_tag`: 现有 release tag；提供时优先回填该 release。
- `head_sha`: 现有已发布提交 SHA；仅当该提交已指向 release tag 时可用。
- 默认：两个输入都为空时，回填 latest published GitHub Release。

### 输出（Output）

- Format: human
- 成功时更新现有 GitHub Release 的资产列表，补齐 `octo-rill-linux-x86_64.tar.gz`。

### 退出码（Exit codes）

- `0`: backfill 成功
- non-zero: 输入无法解析到现有 release，或 bundle/build/release 上传失败

### 兼容性与迁移（Compatibility / migration）

- `workflow_dispatch` 不再承担“为任意 SHA 创建新 release”的职责，只用于现有 release 的资产回填。
- 正常新发版仍由 `workflow_run` 自动路径负责。
