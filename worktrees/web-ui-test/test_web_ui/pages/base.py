"""BasePage: 所有页面对象的基类，提供通用能力."""

import re
import time
from typing import Optional

from playwright.sync_api import Page, Locator, ExpectTimeout

BASE_URL = "http://localhost:8080"


class BasePage:
    """页面对象基类."""

    def __init__(self, page: Page):
        self.page = page
        self.base_url = BASE_URL

    def goto(self, path: str = "/"):
        """导航到指定路径."""
        self.page.goto(f"{self.base_url}{path}")

    def click_nav(self, view_name: str):
        """点击侧边栏导航项.

        Args:
            view_name: nav-item 的 data-view 属性值，如 "dashboard", "proxies"
        """
        self.page.click(f'[data-view="{view_name}"]')
        self.wait_view_active(view_name)

    def wait_view_active(self, view_name: str, timeout: int = 5000):
        """等待指定 view 显示."""
        self.page.wait_for_selector(f"#view-{view_name}.active", timeout=timeout)

    def get_toast(self, timeout: int = 3000) -> Optional[str]:
        """获取当前显示的 toast 文本，无 toast 则返回 None."""
        try:
            toast = self.page.wait_for_selector(".toast.show", timeout=timeout)
            return toast.inner_text()
        except ExpectTimeout:
            return None

    def get_toast_type(self) -> Optional[str]:
        """获取 toast 类型: 'success' 或 'error'."""
        try:
            el = self.page.wait_for_selector(".toast.show", timeout=2000)
            cls = el.get_attribute("class") or ""
            if "success" in cls:
                return "success"
            if "error" in cls:
                return "error"
            return None
        except ExpectTimeout:
            return None

    def wait_toast_hidden(self, timeout: int = 5000):
        """等待 toast 消失."""
        time.sleep(0.3)
        try:
            self.page.wait_for_selector(".toast:not(.show)", timeout=timeout)
        except ExpectTimeout:
            pass

    def get_status_dot_class(self) -> str:
        """获取 header status-dot 的 class 名称."""
        return self.page.get_attribute("#headerStatus", "class") or ""

    def take_screenshot(self, name: str):
        """测试失败时截图保存."""
        self.page.screenshot(path=f"test_web_ui/screenshots/{name}.png")

    def is_view_visible(self, view_name: str) -> bool:
        """检查 view 是否可见."""
        cls = self.page.get_attribute(f"#view-{view_name}", "class") or ""
        return "active" in cls

    def get_page_title(self) -> str:
        """获取页面标题."""
        return self.page.title()

    def reload(self):
        """刷新页面."""
        self.page.reload()
        self.page.wait_for_load_state("domcontentloaded")
