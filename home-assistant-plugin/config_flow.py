"""Config flow for Primare I22 integration."""
from __future__ import annotations

import logging

import aiohttp
import voluptuous as vol
from homeassistant import config_entries
from homeassistant.helpers.aiohttp_client import async_get_clientsession

from .const import (
    DOMAIN,
    CONF_HOST,
    CONF_PORT,
    CONF_SCAN_INTERVAL,
    DEFAULT_PORT,
    DEFAULT_SCAN_INTERVAL,
    DEFAULT_NAME,
)

_LOGGER = logging.getLogger(__name__)

STEP_USER_DATA_SCHEMA = vol.Schema(
    {
        vol.Required(CONF_HOST): str,
        vol.Required(CONF_PORT, default=DEFAULT_PORT): int,
        vol.Optional(CONF_SCAN_INTERVAL, default=DEFAULT_SCAN_INTERVAL): int,
    }
)


class PrimareConfigFlow(config_entries.ConfigFlow, domain=DOMAIN):
    """Handle the config flow for Primare I22."""

    VERSION = 1

    async def async_step_user(self, user_input=None):
        """Handle the initial step shown in the HA UI."""
        if user_input is not None:
            host = user_input[CONF_HOST]
            port = user_input[CONF_PORT]
            base_url = f"http://{host}:{port}"
            title = DEFAULT_NAME

            # Best effort: use device info when reachable, but allow offline setup.
            try:
                session = async_get_clientsession(self.hass)
                async with session.get(
                    f"{base_url}/info",
                    timeout=aiohttp.ClientTimeout(total=5),
                ) as resp:
                    resp.raise_for_status()
                    info = await resp.json()
                    model = info.get("model", DEFAULT_NAME)
                    title = f"Primare {model}"
            except Exception:
                _LOGGER.warning(
                    "Primare endpoint not reachable during setup at %s; creating entry anyway",
                    base_url,
                )

            # Prevent duplicate entries for the same host
            await self.async_set_unique_id(f"{host}:{port}")
            self._abort_if_unique_id_configured()

            return self.async_create_entry(title=title, data=user_input)

        return self.async_show_form(
            step_id="user",
            data_schema=STEP_USER_DATA_SCHEMA,
            errors={},
            description_placeholders={
                "default_port": str(DEFAULT_PORT),
            },
        )
