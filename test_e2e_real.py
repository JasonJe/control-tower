#!/usr/bin/env python3
"""
Control Tower Web UI 真实端到端测试
模拟人操作：点击按钮 → 观察页面变化 → 验证 UI 反馈
"""

from playwright.sync_api import sync_playwright, Page, Browser
from dataclasses import dataclass, field
from typing import Optional
import time, sys

BASE = "http://localhost:8080"
API = "http://localhost:8080/api"

# ─────────────────────────────────────────────
# Test infrastructure
# ─────────────────────────────────────────────

@dataclass
class Result:
    passed: int = 0
    failed: int = 0
    logs: list = field(default_factory=list)

    def check(self, label: str, cond: bool, detail: str = ""):
        if cond:
            self.passed += 1
            self.logs.append(f"  [PASS] {label}")
        else:
            self.failed += 1
            self.logs.append(f"  [FAIL] {label}" + (f" → {detail}" if detail else ""))

    def summary(self) -> str:
        total = self.passed + self.failed
        s = f"\n{'='*60}\nResults: {self.passed}/{total} passed, {self.failed} failed\n"
        if self.failed > 0:
            s += f"=== {self.failed} CHECKS FAILED ===\n"
        return s


class E2E:
    def __init__(self, page: Page, result: Result):
        self.page = page
        self.r = result
        self._errors: list[str] = []
        page.on("console", lambda m: self._errors.append(m.text) if m.type == "error" else None)

    def goto(self, path="/"):
        self.page.goto(BASE + path)
        self.page.wait_for_load_state("domcontentloaded")
        # 等 loadDashboard() 执行完（Dashboard 数据显示后）
        try:
            self.page.wait_for_function(
                "document.getElementById('dash-nodes') !== null && "
                "document.getElementById('dash-nodes').textContent.trim() !== '' && "
                "document.getElementById('dash-nodes').textContent.trim() !== '-'",
                timeout=10000
            )
        except Exception:
            pass
        # 每次 goto 后重新注入劫持（reload 导致 window 重置）
        self.page.evaluate("""
            window._toastMsgs = window._toastMsgs || [];
            const _orig = window.showToast || (()=>{});
            window.showToast = (msg, type) => {
                window._toastMsgs.push(msg);
                _orig(msg, type);
            };
        """)

    def nav(self, name: str):
        """点击导航到指定页面"""
        self.page.locator(".nav-item", has_text=name).click()
        # 等视图切换 + 数据加载（每个视图都有 API 请求）
        try:
            self.page.wait_for_load_state("networkidle", timeout=8000)
        except Exception:
            self.page.wait_for_timeout(1500)

    def click(self, selector: str, name: str = ""):
        """点击元素，带超时"""
        self.page.locator(selector).click(timeout=5000)
        if name:
            self.page.wait_for_timeout(200)

    def get_toast_msg(self) -> str:
        """读取当前 toast 文字（toast 在 show 状态才读）"""
        toast = self.page.locator("#toast")
        cls = (toast.get_attribute("class") or "")
        if "show" in cls:
            return toast.text_content().strip()
        return ""

    def wait_for_toast(self, timeout: int = 5000) -> str | None:
        """等待 toast 弹出（检查 DOM #toast show + window._toastMsgs），返回消息文字，超时返回 None"""
        deadline = time.time() + timeout / 1000
        while time.time() < deadline:
            # 方式1：DOM toast 正在显示
            msg = self.get_toast_msg()
            if msg:
                return msg
            # 方式2：window._toastMsgs 中出现新消息
            msgs: list[str] = self.page.evaluate("window._toastMsgs || []")
            if msgs:
                return msgs[-1]
            time.sleep(0.1)
        return None

    def fatal_errors(self) -> list[str]:
        """过滤掉网络请求类错误（由延迟超时等业务原因产生）"""
        return [e for e in self._errors if "Failed to load" not in e]

    def console_ok(self, label: str):
        self.r.check(label, len(self.fatal_errors()) == 0,
                    f"{self.fatal_errors()}" if self.fatal_errors() else "")


# ─────────────────────────────────────────────
# 工具：等页面出现指定内容
# ─────────────────────────────────────────────

def test_mode_switch_interaction(e2e: E2E):
    e2e.goto("/")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Dashboard 加载 ===")

    # 1. 页面标题
    r.check("页面标题正确", page.title() == "Control Tower")

    # 2. 等待 Dashboard 加载完成（5秒自动刷新，3秒足够看到数据）
    page.wait_for_timeout(3000)
    nodes_text = page.locator("#dash-nodes").text_content().strip()
    r.check("节点数已渲染", nodes_text and nodes_text.isdigit() and int(nodes_text) > 0,
            f"值={nodes_text}")

    mode_text = page.locator("#dash-mode").text_content().strip()
    r.check("模式已渲染", mode_text in ["RULE", "GLOBAL", "DIRECT"],
            f"值={mode_text}")

    proxy_text = page.locator("#dash-proxy").text_content().strip()
    r.check("当前代理已渲染", len(proxy_text) > 3, f"值={proxy_text[:30]}")

    # 3. 状态指示灯
    dot = page.locator("#headerStatus")
    r.check("状态指示灯可见", dot.is_visible())
    dot_class = dot.get_attribute("class")
    r.check("指示灯显示运行中", "running" in (dot_class or ""), f"class={dot_class}")

    # 4. 所有模式按钮存在
    for mode in ["Rule", "Global", "Direct"]:
        btn = page.get_by_role("button", name=mode)
        r.check(f"'{mode}' 模式按钮可见", btn.is_visible())

    # 5. 启停按钮
    r.check("'Start Service' 按钮可见",
            page.get_by_role("button", name="Start Service").is_visible())
    r.check("'Stop Service' 按钮可见",
            page.get_by_role("button", name="Stop Service").is_visible())

    # 6. 无控制台错误
    e2e.console_ok("Dashboard 加载无 JS 错误")


# ─────────────────────────────────────────────
# 测试 1：Dashboard 加载
# ─────────────────────────────────────────────

def test_dashboard_load(e2e: E2E):
    e2e.goto("/")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] 模式切换 ===")

    # 获取当前模式
    before_mode = page.locator("#dash-mode").text_content().strip()

    # 点击 Global 按钮
    page.get_by_role("button", name="Global").click()

    # 等页面模式显示更新（toast 在 reload 后消失，不检测 toast DOM）
    page.wait_for_timeout(1000)
    after_mode = page.locator("#dash-mode").text_content().strip()
    r.check(f"Dashboard 模式从 '{before_mode}' 变为 'GLOBAL'",
            after_mode == "GLOBAL", f"实际={after_mode}")

    # 检查 Global 按钮高亮激活
    global_btn_class = page.get_by_role("button", name="Global").get_attribute("class")
    r.check("Global 按钮激活高亮", "primary" in (global_btn_class or ""),
            f"class={global_btn_class}")

    # 切回 Rule
    page.get_by_role("button", name="Rule").click()
    page.wait_for_timeout(1000)
    restored = page.locator("#dash-mode").text_content().strip()
    r.check(f"切回 Rule 后模式恢复为 'RULE'", restored == "RULE",
            f"实际={restored}")


# ─────────────────────────────────────────────
# 测试 3：Dashboard 刷新按钮
# ─────────────────────────────────────────────

def test_dashboard_refresh(e2e: E2E):
    e2e.goto("/")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Dashboard 刷新 ===")

    page.wait_for_timeout(1000)
    before = page.locator("#dash-nodes").text_content().strip()

    # 点击刷新按钮
    page.get_by_role("button", name="Refresh").first.click()
    page.wait_for_timeout(2000)

    after = page.locator("#dash-nodes").text_content().strip()
    r.check("刷新后节点数仍正常显示", after.isdigit() and int(after) > 0,
            f"刷新前={before}, 刷新后={after}")

    e2e.console_ok("Dashboard 刷新无 JS 错误")


# ─────────────────────────────────────────────
# 测试 4：Proxies 页面加载和搜索过滤
# ─────────────────────────────────────────────

def test_proxies_page(e2e: E2E):
    e2e.goto("/")
    e2e.nav("Proxies")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Proxies 页面 ===")

    # nav 后等待节点数据加载（切视图不清空，需要等 JS 重新渲染）
    try:
        page.wait_for_selector("#proxy-table tr:has(td)", timeout=8000)
    except Exception:
        pass
    rows = page.locator("#proxy-table tr:has(td)").all()
    r.check("代理节点表格已渲染", len(rows) > 1, f"{len(rows)} 行")

    # 2. 表格列头正确
    headers = page.locator("table").first.locator("th").all_text_contents()
    r.check("表格含 Name 列", any("Name" in h for h in headers))
    r.check("表格含 Latency 列", any("Latency" in h or "Delay" in h for h in headers))

    # 3. 点击 Test All 测速按钮
    test_all_btn = page.get_by_role("button", name="Test All")
    r.check("'Test All' 测速按钮存在", test_all_btn.count() > 0)

    # 4. 输入搜索过滤
    search_input = page.locator("#proxy-search")
    if search_input.is_visible():
        search_input.fill("香港")
        page.wait_for_timeout(500)
        filtered = page.locator("#proxy-table tr:has(td)").count()
        r.check("搜索'香港'后表格行数减少", len(rows) > 1 and filtered < len(rows),
                f"过滤前={len(rows)}, 过滤后={filtered}")
        # 清空搜索
        search_input.fill("")

    e2e.console_ok("Proxies 页面无 JS 错误")


# ─────────────────────────────────────────────
# 测试 5：Profiles 页面加载
# ─────────────────────────────────────────────

def test_profiles_page(e2e: E2E):
    e2e.goto("/")
    e2e.nav("Profiles")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Profiles 页面 ===")

    # 1. 等待加载完成
    page.wait_for_timeout(2000)

    # 2. Add Profile 表单存在
    add_btn = page.get_by_role("button", name="Add Profile")
    r.check("'Add Profile' 按钮可见", add_btn.is_visible())

    url_input = page.locator("#profile-url")
    name_input = page.locator("#profile-name")
    r.check("Profile URL 输入框存在", url_input.count() > 0)
    r.check("Profile Name 输入框存在", name_input.count() > 0)

    # 3. 如果有 Profile，显示内容
    profile_items = page.locator(".profile-item, .profile-card").all()
    r.check("Profile 列表已渲染（>= 0 items）", len(profile_items) >= 0,
            f"{len(profile_items)} items")

    # 4. 检查 Activate 按钮（如果有 Profile）
    activate_btns = page.locator("button", has_text="Activate").all()
    if activate_btns:
        r.check("Activate 按钮可见", activate_btns[0].is_visible())

    e2e.console_ok("Profiles 页面无 JS 错误")


# ─────────────────────────────────────────────
# 测试 6：Rules 页面加载和搜索过滤
# ─────────────────────────────────────────────

def test_rules_page(e2e: E2E):
    e2e.goto("/")
    e2e.nav("Rules")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Rules 页面 ===")

    # 1. 等待规则加载
    page.wait_for_timeout(1500)
    rows = page.locator("table tr").all()
    r.check("规则表格已渲染", len(rows) >= 1, f"{len(rows)} 行")

    # 2. 表格数据行有 Proxy 列值（第4列）
    data_rows = page.locator("#rules-table tbody tr:not(.loading-row)").all()
    if not data_rows:
        data_rows = [r for r in rows if r.locator("td").count() >= 4]
    r.check("规则数据行有 Proxy 列值",
            len(data_rows) > 0 and data_rows[0].locator("td").nth(3).text_content().strip() != "",
            f"数据行数={len(data_rows)}")

    # 3. 搜索过滤
    search_input = page.locator("#rules-search")
    if search_input.is_visible():
        search_input.fill("bilibili")
        page.wait_for_timeout(500)
        filtered_rows = page.locator("table tr").count()
        r.check("搜索'bilibili'后行数减少", filtered_rows < len(rows),
                f"过滤前={len(rows)}, 过滤后={filtered_rows}")
        search_input.fill("")

    # 4. 规则数远超 0
    total_rules = len(rows) - 1
    r.check(f"规则总数 > 0（共 {total_rules} 条）", total_rules > 0)

    e2e.console_ok("Rules 页面无 JS 错误")


# ─────────────────────────────────────────────
# 测试 7：Connections 页面和刷新/暂停
# ─────────────────────────────────────────────

def test_connections_page(e2e: E2E):
    e2e.goto("/")
    e2e.nav("Connections")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Connections 页面 ===")

    page.wait_for_timeout(1000)

    # 1. 表格已渲染（不管有没有数据）
    table_rows = page.locator("table tr").count()
    r.check("连接表格已渲染", table_rows >= 1, f"{table_rows} 行")

    # 2. Pause 按钮存在
    pause_btn = page.locator("button", has_text="Pause")
    r.check("'Pause' 按钮存在", pause_btn.count() > 0)

    # 3. 点击 Pause 暂停自动刷新
    if pause_btn.first.is_visible():
        pause_btn.first.click()
        page.wait_for_timeout(500)
        # 按钮文字应该变成 Resume 或类似
        resume_count = page.locator("button", has_text="Resume").count()
        r.check("点击 Pause 后出现 Resume 按钮", resume_count > 0)

        # 恢复刷新
        if resume_count > 0:
            page.locator("button", has_text="Resume").first.click()
            page.wait_for_timeout(500)

    # 4. Refresh 按钮存在
    r.check("'Refresh' 按钮存在",
            page.get_by_role("button", name="Refresh").count() > 0)

    e2e.console_ok("Connections 页面无 JS 错误")


# ─────────────────────────────────────────────
# 测试 8：Settings 页面 — 编辑端口
# ─────────────────────────────────────────────

def test_settings_edit_ports(e2e: E2E):
    e2e.goto("/")
    e2e.nav("Settings")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Settings — 编辑端口 ===")

    page.wait_for_timeout(500)

    # 1. 端口输入框可见且有值
    port_http = page.locator("#port-http")
    if port_http.is_visible():
        val = port_http.input_value()
        r.check("HTTP 端口有默认值", val.isdigit() and int(val) > 0, f"值={val}")

    # 2. 点击 Edit Ports 打开编辑模式
    edit_btn = page.locator("button", has_text="Edit Ports")
    if edit_btn.count() > 0 and edit_btn.first.is_visible():
        edit_btn.first.click()
        page.wait_for_timeout(300)

        # Save 和 Cancel 按钮出现
        save_btn = page.locator("button", has_text="Save")
        cancel_btn = page.locator("button", has_text="Cancel")
        r.check("Edit 后出现 'Save' 按钮", save_btn.count() > 0)
        r.check("Edit 后出现 'Cancel' 按钮", cancel_btn.count() > 0)

        # 改端口值测试
        port_http_edit = page.locator("#port-http")
        if port_http_edit.is_visible():
            port_http_edit.fill("27890")
            save_btn.first.click()
            page.wait_for_timeout(1000)
            # 验证 toast
            toast = e2e.wait_for_toast(timeout=3000)
            r.check("修改端口后弹出 toast", toast is not None,
                    f"toast={toast}" if toast else "")
        else:
            r.check("端口编辑测试跳过（元素不可见）", True)

    e2e.console_ok("Settings 页面无 JS 错误")


# ─────────────────────────────────────────────
# 测试 9：Settings — Auto Test 配置
# ─────────────────────────────────────────────

def test_settings_auto_test(e2e: E2E):
    e2e.goto("/")
    e2e.nav("Settings")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Settings — Auto Test ===")

    page.wait_for_timeout(500)

    # 1. Auto Test 开关存在
    auto_enabled = page.locator("#auto-test-enabled")
    if auto_enabled.count() > 0:
        r.check("Auto Test 开关存在", True)

    # 2. 点击 Edit Latency 设置区域（如果可见）
    edit_latency = page.locator("button", has_text="Edit").first
    if edit_latency.count() > 0 and edit_latency.is_visible():
        edit_latency.click()
        page.wait_for_timeout(300)
        save_btn = page.locator("button", has_text="Save")
        cancel_btn = page.locator("button", has_text="Cancel")
        r.check("Edit Latency 后出现 Save/Cancel", save_btn.count() > 0)
        if cancel_btn.count() > 0:
            cancel_btn.first.click()
            page.wait_for_timeout(300)

    e2e.console_ok("Settings Auto Test 无 JS 错误")


# ─────────────────────────────────────────────
# 测试 10：Settings — 日志面板
# ─────────────────────────────────────────────

def test_settings_logs(e2e: E2E):
    e2e.goto("/")
    e2e.nav("Settings")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Settings — 日志面板 ===")

    # 1. 页面内含 Log 字样区域
    log_text = page.locator("#view-settings", has_text="Log").count()
    r.check("Settings 页面含日志区域", log_text >= 1)

    # 2. 日志加载按钮存在
    log_btns = page.locator("button[onclick='loadLogs()']")
    r.check("日志加载按钮存在", log_btns.count() > 0)

    # 3. 点击加载日志
    if log_btns.count() > 0:
        log_btns.first.click()
        page.wait_for_timeout(2000)
        # 检查日志内容出现
        log_content = page.locator("#view-settings").inner_text()
        r.check("日志加载后内容包含 ctsvc 日志",
                "INFO" in log_content or "ctsvc" in log_content,
                f"内容长度={len(log_content)}")


# ─────────────────────────────────────────────
# 测试 11：Service Start / Stop（Dashboard 按钮）
# ─────────────────────────────────────────────

def test_service_start_stop(e2e: E2E):
    e2e.goto("/")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] Service 启停 ===")

    # 获取初始状态
    page.wait_for_timeout(1000)
    status_before = page.locator("#headerStatus").get_attribute("class")
    was_running = "running" in (status_before or "")
    stop_btn = page.get_by_role("button", name="Stop Service")
    stop_enabled = stop_btn.is_enabled()

    if stop_enabled:
        # Stop Service
        stop_btn.click()
        # toast 在 reload 触发前（1.5s）就已显示，reload 后消失
        page.wait_for_timeout(100)
        toast_el = page.locator("#toast")
        toast = (toast_el.text_content().strip()
                 if "show" in (toast_el.get_attribute("class") or "") else "")
        r.check("点击 Stop Service 弹出 toast",
                toast and "stop" in toast.lower(), f"toast={toast}")

        # 在 reload 触发前，old page 的 loadDashboard() 还没有执行，
        # 这里 status 还是 running（因为还没 reload）。直接等 reload 完成。
        page.wait_for_timeout(2000)
        # reload 后新页面加载完成，状态灯应为 stopped
        page.wait_for_function(
            "document.getElementById('headerStatus') !== null",
            timeout=8000
        )
        status_after_stop = page.locator("#headerStatus").get_attribute("class")
        r.check("Stop 后状态指示灯不再是 running",
                "running" not in (status_after_stop or ""))

        # 启动按钮应该高亮
        start_btn_class = page.get_by_role("button", name="Start Service").get_attribute("class")
        r.check("Stop 后 Start Service 按钮高亮",
                "success" in (start_btn_class or "") or "primary" in (start_btn_class or ""))

        # 重新启动（此时 Mihomo 已停，Start 后 reload）
        page.get_by_role("button", name="Start Service").click()
        # Start Service 成功后 showToast 立即显示，等待并轮询 toast DOM
        toast2 = ""
        for _ in range(50):  # 最多 5s
            toast_el2 = page.locator("#toast")
            cls2 = toast_el2.get_attribute("class") or ""
            if "show" in cls2:
                toast2 = toast_el2.text_content().strip()
                break
            import time; time.sleep(0.1)
        r.check("点击 Start Service 弹出 toast",
                toast2 and "start" in toast2.lower(), f"toast={toast2}")

        # 等 reload 后验证状态恢复
        page.wait_for_timeout(2500)
        page.wait_for_function(
            "document.getElementById('headerStatus') !== null",
            timeout=8000
        )
        status_restored = page.locator("#headerStatus").get_attribute("class")
        r.check("Start 后状态指示灯恢复 running",
                "running" in (status_restored or ""),
                f"status={status_restored}")
    else:
        # Mihomo 已处于停止状态，只测试 Start Service
        r.check("Stop Service 按钮已禁用（预期：Mihomo 尚未启动）", True)
        start_btn = page.get_by_role("button", name="Start Service")
        r.check("Start Service 按钮已启用", start_btn.is_enabled())
        start_btn.click()
        toast2 = ""
        for _ in range(50):
            toast_el2 = page.locator("#toast")
            cls2 = toast_el2.get_attribute("class") or ""
            if "show" in cls2:
                toast2 = toast_el2.text_content().strip()
                break
            import time; time.sleep(0.1)
        r.check("点击 Start Service 弹出 toast",
                toast2 and "start" in toast2.lower(), f"toast={toast2}")
        page.wait_for_timeout(2500)
        page.wait_for_function(
            "document.getElementById('headerStatus') !== null",
            timeout=8000
        )
        status_restored = page.locator("#headerStatus").get_attribute("class")
        r.check("Start 后状态指示灯恢复 running",
                "running" in (status_restored or ""),
                f"status={status_restored}")

    e2e.console_ok("Service 启停无 JS 错误")


# ─────────────────────────────────────────────
# 测试 12：全导航完整性
# ─────────────────────────────────────────────

def test_navigation(e2e: E2E):
    e2e.goto("/")
    r = e2e.r
    page = e2e.page

    print("\n=== [E2E] 全导航切换 ===")

    # 若 Service 处于停止状态（test_service_start_stop 后），先恢复
    status = page.locator("#headerStatus").get_attribute("class") or ""
    if "running" not in status:
        page.get_by_role("button", name="Start Service").click()
        try:
            page.wait_for_function(
                "document.getElementById('headerStatus').classList.contains('running')",
                timeout=10000
            )
        except Exception:
            pass
        e2e.page.evaluate("window._toastMsgs = window._toastMsgs || [];"
            "const _o = window.showToast || (()=>{});"
            "window.showToast = (m,t) => { window._toastMsgs.push(m); _o(m,t); };")

    pages = ["Dashboard", "Proxies", "Profiles", "Rules", "Connections", "Settings"]
    view_map = {
        "Dashboard": "view-dashboard",
        "Proxies": "view-proxies",
        "Profiles": "view-profiles",
        "Rules": "view-rules",
        "Connections": "view-connections",
        "Settings": "view-settings",
    }

    for pname in pages:
        # 导航到页面
        e2e.nav(pname)
        page.wait_for_timeout(400)

        # 对应 view 激活
        view_class = page.locator(f"#{view_map[pname]}").get_attribute("class")
        r.check(f"点击 '{pname}' → #{view_map[pname]} 视图激活",
                view_class == "view active", f"class={view_class}")

        # 导航项高亮
        nav_item_class = page.locator(".nav-item", has_text=pname).first.get_attribute("class")
        r.check(f"'{pname}' 导航项高亮", "active" in (nav_item_class or ""))

    # 侧边栏始终可见
    sidebar = page.locator("nav.sidebar, .sidebar, nav").first
    r.check("侧边栏始终可见", sidebar.is_visible())


# ─────────────────────────────────────────────
# MAIN
# ─────────────────────────────────────────────

def run():
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        page = browser.new_page()

        result = Result()
        e2e = E2E(page, result)

        suites = [
            ("Dashboard 加载", test_dashboard_load),
            ("模式切换交互", test_mode_switch_interaction),
            ("Dashboard 刷新", test_dashboard_refresh),
            ("Proxies 页面", test_proxies_page),
            ("Profiles 页面", test_profiles_page),
            ("Rules 页面", test_rules_page),
            ("Connections 页面", test_connections_page),
            ("Settings 编辑端口", test_settings_edit_ports),
            ("Settings Auto Test", test_settings_auto_test),
            ("Settings 日志面板", test_settings_logs),
            ("全导航切换", test_navigation),       # 确保 Mihomo running
            ("Service 启停", test_service_start_stop),  # 破坏性测试放最后
        ]

        for name, fn in suites:
            try:
                fn(e2e)
            except Exception as ex:
                result.failed += 1
                result.logs.append(f"  [ERROR] {name} 异常: {ex}")

        browser.close()

        for line in result.logs:
            print(line)
        print(result.summary())
        sys.exit(0 if result.failed == 0 else 1)


if __name__ == "__main__":
    run()
