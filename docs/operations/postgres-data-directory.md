# PostgreSQL 数据目录契约

本文记录 PostgreSQL 数据目录在宿主机与容器之间的所有权约定，以及违反该约定时的故障形态。它约束的是**部署配置**；应用行为见 `docs/specs/postgresql-runtime/`。

定位：`docs/operations/postgres-data-directory.md`

## 一句话

**数据目录里每个文件的所有者 uid 必须等于容器内 PostgreSQL 进程的 uid。** 否则 PostgreSQL 会在运行一段时间后开始失败的读写，而健康检查仍然报 `healthy`。

## 为什么这条约定存在

容器通过 bind mount 暴露数据目录：

```yaml
volumes:
  - /path/to/postgres-data:/var/lib/postgresql
```

Docker 不会为 bind mount 调整所有者。官方 `postgres` 镜像的 entrypoint 只在**数据目录为空**时执行 `initdb` 并 chown；已有数据时它完全跳过所有权处理，直接 `exec` 成 postgres 用户。

因此所有权是部署方的责任，镜像不提供兜底。

## uid 由镜像决定

不同基础镜像的 `postgres` 用户 uid 不同，不能假设：

```bash
docker run --rm postgres:18-alpine      sh -c 'id postgres'   # uid=70
docker run --rm postgres:18             sh -c 'id postgres'   # uid=999
```

改动基底镜像（例如从 `postgres:18` 换成 `postgres:18-alpine`）会改变这个 uid，从而打破约定。切换镜像时必须重新核对并 chown。

## 故障形态：它是静默的

违反约定后 PostgreSQL **不会立即失败**。已建立的连接持有打开的文件描述符，后续读写走内存中的 fd，不重新 `open(2)`。只有当某次操作需要新开文件时才撞墙：

```
ERROR:  could not open file "global/pg_filenode.map": Permission denied
ERROR:  could not open directory "pg_logical/snapshots": Permission denied
```

可能潜伏数天。触发因素包括 checkpoint 写盘、WAL 段切换、新连接、目录扫描。

### 为什么健康检查发现不了

`pg_isready` 只验证服务端接受了 socket 连接与协议握手，不验证读写：

```yaml
healthcheck:
  test: ["CMD-SHELL", "pg_isready -U $$POSTGRES_USER -d $$POSTGRES_DB"]
```

线上曾经在 **47,737 次** 文件访问失败的同时持续报告 `healthy` 超过 44 小时。依赖健康状态的 `depends_on: condition: service_healthy` 因此把应用容器放行了 12 次重启。

## 该做什么

### 建立或迁移数据目录后

```bash
# 先取容器内真实 uid，不要猜
UID_IN_CONTAINER=$(docker run --rm postgres:18-alpine id -u postgres)

# 停库，再改，避免并发写
docker compose stop postgres
docker run --rm -v /path/to/postgres-data:/data --user 0:0 alpine \
  sh -c "chown -R ${UID_IN_CONTAINER}:${UID_IN_CONTAINER} /data/18/docker"
docker compose start postgres
```

注意数据根是 `<mount>/<major>/docker`（例如 `/var/lib/postgresql/18/docker`），不是挂载点本身。

### 注意保持所有权的操作

`mv`、`cp -a`、从备份解包、以其他用户身份创建目录，都会保留或引入非预期 uid。**移动数据目录后必须重新核对所有权。**

### 校验

```bash
docker exec <pg-container> sh -c 'test -w /var/lib/postgresql/18/docker && echo writable'
```

以及比较宿主与容器两侧：

```bash
stat -c '%u:%g' /path/to/postgres-data/18/docker
docker exec <pg-container> id -u postgres
```

两者必须相等。

## 健康检查应当覆盖写入

只测连通性的探针无法区分「能连」与「能干活」。一个能发现所有权故障的最小探针：

```yaml
healthcheck:
  test:
    - CMD-SHELL
    - >-
      pg_isready -U $$POSTGRES_USER -d $$POSTGRES_DB &&
      psql -U $$POSTGRES_USER -d $$POSTGRES_DB -tAc 'SELECT 1' >/dev/null
  interval: 5s
  timeout: 5s
  retries: 30
```

`SELECT 1` 仍可能命中纯缓存路径，因此它是改进而非证明。更强的做法是周期性写入临时表；是否值得这笔开销由部署方权衡。

## 与镜像更新的关系

生产栈由 Dockrev 管理镜像 digest，它通过 `managed-overrides` 覆盖 compose，**不修改仓库或服务器上的 `docker-compose.yml`**。后果：

- 源文件里的 `image:` digest 与运行时不一致是正常的；
- 手工 `docker compose up -d` 不会采用 override，会**回滚到源文件里的旧 digest**。

任何需要手工干预的操作前后，都必须检查实际运行的 digest：

```bash
docker inspect <app-container> --format '{{.Image}}'
```

## 排查顺序

看到 `Permission denied` 时：

1. 取容器内 uid 与数据目录属主，先比较这两个数
2. 不要只重启容器——重启不改所有权，只是重新撞墙
3. 确认没有其他进程正在写该目录，再执行 chown
4. chown 后观察日志里是否还有 `Permission denied`，不能只看容器状态
