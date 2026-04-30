# Control Tower Docker 部署

## 快速开始

```bash
# 构建镜像
docker-compose build

# 启动服务
docker-compose up -d

# 查看日志
docker-compose logs -f

# 停止服务
docker-compose down
```

## 端口说明

| 端口 | 服务 | 说明 |
|------|------|------|
| 9090 | Mihomo API | Clash API 控制接口 |
| 7890 | HTTP Proxy | HTTP 代理端口 |
| 7891 | SOCKS5 Proxy | SOCKS5 代理端口 |
| 8080 | Web UI | Web 管理界面 |

## 数据持久化

 volumes 挂载点:

- `./data/config` -> `/app/config` - 配置文件
- `./data/profiles` -> `/app/profiles` - 订阅配置
- `./data/logs` -> `/tmp/logs` - 日志文件

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| TZ | Asia/Shanghai | 时区设置 |

## Web UI 访问

启动后访问: http://localhost:8080

## CLI 使用

```bash
# 进入容器
docker exec -it control-tower sh

# 使用 ctctl
ctctl proxy list
ctctl service status
```

## 构建独立镜像

```bash
# 仅构建镜像
docker build -t control-tower:latest .

# 运行
docker run -d \
  --name control-tower \
  -p 9090:9090 \
  -p 7890:7890 \
  -p 7891:7891 \
  -p 8080:8080 \
  -v $(pwd)/data:/app \
  control-tower:latest
```
