"""pytest configuration: ctsvc lifecycle + browser fixtures."""

import os
import signal
import subprocess
import time
from pathlib import Path

import pytest
from playwright.sync_api import sync_playwright, Browser, BrowserContext, Page

# ctsvc dist 路径
CTSVC_PATH = Path("/home/jason/tui-clash-verge/control-tower/target/dist/control-tower/ctsvc")
BASE_URL = "http://localhost:8080"


def is_ctsvc_ready(port: int = 8080, timeout: int = 15) -> bool:
    """检查 HTTP 端口是否开始监听."""
    import socket
    start = time.time()
    while time.time() - start < timeout:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=1):
                return True
        except OSError:
            time.sleep(0.5)
    return False


@pytest.fixture(scope="session")
def ctsvc_process():
    """启动 ctsvc 进程，session 结束时自动清理."""
    ctsvc_dir = CTSVC_PATH.parent

    # 确保 config.yaml 存在（供 Mihomo 启动用）
    config_yaml = ctsvc_dir / "config.yaml"
    if not config_yaml.exists():
        config_yaml.write_text("port: 7890\nsocks-port: 7891\napi-port: 9090\nallow-lan: false\nmode: rule\nlog-level: info\n")

    # 清理可能存在的残留进程
    try:
        result = subprocess.run(["pkill", "-9", "-f", "ctsvc"], capture_output=True)
    except Exception:
        pass
    time.sleep(1)

    # 启动 ctsvc
    env = os.environ.copy()
    proc = subprocess.Popen(
        [str(CTSVC_PATH)],
        cwd=str(ctsvc_dir),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        preexec_fn=os.setsid,
    )

    # 等待 HTTP server 就绪
    if not is_ctsvc_ready(8080, timeout=20):
        os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
        pytest.fail("ctsvc HTTP server failed to start within 20s")

    # 等待 Mihomo 可通过 API 访问
    time.sleep(1)

    yield proc

    # Teardown: 杀整个进程组
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
    except OSError:
        pass

    # 清理可能的 mihomo 残留
    try:
        subprocess.run(["pkill", "-9", "-f", "mihomo"], capture_output=True)
    except Exception:
        pass

    # 确保端口释放
    time.sleep(1)


@pytest.fixture(scope="session")
def browser(ctsvc_process):
    """Playwright browser 实例，session 级别共享."""
    with sync_playwright() as p:
        b = p.chromium.launch(headless=True)
        yield b
        b.close()


@pytest.fixture(scope="session")
def context(browser: Browser, ctsvc_process):
    """独立浏览器上下文."""
    ctx = browser.new_context()
    yield ctx
    ctx.close()


@pytest.fixture(scope="function")
def page(context: BrowserContext, ctsvc_process) -> Page:
    """每个测试函数一个新页面."""
    p = context.new_page()
    p.goto(BASE_URL)
    yield p
    p.close()


@pytest.fixture(scope="function")
def service_running(page: Page):
    """确保 Mihomo 已启动（调用 service start API）."""
    page.request.post(f"{BASE_URL}/api/service/start")
    time.sleep(2)
    yield
    page.request.post(f"{BASE_URL}/api/service/stop")
    time.sleep(1)
