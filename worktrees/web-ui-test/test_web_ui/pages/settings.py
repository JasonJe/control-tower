"""Settings page object."""

from .base import BasePage


class SettingsPage(BasePage):
    """Settings 页面."""

    BTN_RESTART = "button:has-text('Restart Service')"
    BTN_STOP = "button:has-text('Stop Service')"
    PORT_HTTP = "#port-http"
    PORT_SOCKS5 = "#port-socks5"
    PORT_API = "#port-api"
    VERSION = "#view-settings .mono"

    def goto(self):
        self.page.goto(f"{self.base_url}/")
        self.click_nav("settings")

    def click_restart(self):
        self.page.click(self.BTN_RESTART)

    def click_stop(self):
        self.page.click(self.BTN_STOP)

    def get_http_port(self) -> str:
        return self.page.text_content(self.PORT_HTTP)

    def get_socks5_port(self) -> str:
        return self.page.text_content(self.PORT_SOCKS5)

    def get_api_port(self) -> str:
        return self.page.text_content(self.PORT_API)

    def get_version(self) -> str:
        return self.page.text_content(self.VERSION)
