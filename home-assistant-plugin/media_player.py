"""Primare I22 media_player entity."""
from __future__ import annotations

import logging
from datetime import timedelta

import aiohttp
from homeassistant.components.media_player import (
    MediaPlayerEntity,
    MediaPlayerEntityFeature,
    MediaPlayerState,
)
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.aiohttp_client import async_get_clientsession
from homeassistant.helpers.entity import DeviceInfo
from homeassistant.helpers.entity_platform import AddEntitiesCallback
from homeassistant.helpers.event import async_track_time_interval

from .const import DOMAIN, CONF_HOST, CONF_PORT, CONF_SCAN_INTERVAL, DEFAULT_SCAN_INTERVAL

_LOGGER = logging.getLogger(__name__)

VOLUME_MAX = 79  # I22 range is 0-79, HA expects 0.0-1.0

SUPPORTED_FEATURES = (
    MediaPlayerEntityFeature.TURN_ON
    | MediaPlayerEntityFeature.TURN_OFF
    | MediaPlayerEntityFeature.VOLUME_SET
    | MediaPlayerEntityFeature.VOLUME_STEP
    | MediaPlayerEntityFeature.VOLUME_MUTE
    | MediaPlayerEntityFeature.SELECT_SOURCE
)


async def async_setup_entry(
    hass: HomeAssistant,
    entry: ConfigEntry,
    async_add_entities: AddEntitiesCallback,
) -> None:
    host = entry.data[CONF_HOST]
    port = entry.data[CONF_PORT]
    base_url = f"http://{host}:{port}"
    session = async_get_clientsession(hass)
    info: dict = {}

    # Best effort: if device/service is offline at setup time, still add entity as unavailable.
    try:
        async with session.get(f"{base_url}/info", timeout=aiohttp.ClientTimeout(total=5)) as r:
            r.raise_for_status()
            info = await r.json()
    except Exception as err:
        _LOGGER.warning("Cannot fetch device info from %s during setup: %s", base_url, err)

    async_add_entities([PrimareMediaPlayer(hass, entry, session, base_url, info)])


class PrimareMediaPlayer(MediaPlayerEntity):
    """Primare I22 as a HA media_player. Polls the REST service directly."""

    _attr_has_entity_name = True
    _attr_name = None
    _attr_supported_features = SUPPORTED_FEATURES

    def __init__(
        self,
        hass: HomeAssistant,
        entry: ConfigEntry,
        session: aiohttp.ClientSession,
        base_url: str,
        info: dict,
    ) -> None:
        self.hass = hass
        self._session = session
        self._base_url = base_url
        self._status: dict = {}
        self._input_names: dict[int, str] = {}
        self._attr_unique_id = f"{entry.data[CONF_HOST]}_{entry.data[CONF_PORT]}"
        self._scan_interval = timedelta(seconds=entry.data.get(CONF_SCAN_INTERVAL, DEFAULT_SCAN_INTERVAL))

        model = info.get("model", "I22")
        self._attr_device_info = DeviceInfo(
            identifiers={(DOMAIN, self._attr_unique_id)},
            name=f"Primare {model}",
            manufacturer="Primare",
            model=model,
            sw_version=info.get("firmware"),
        )

    async def async_added_to_hass(self) -> None:
        await self._refresh()
        self._input_names = await self._fetch_input_names()
        self.async_on_remove(
            async_track_time_interval(self.hass, self._poll, self._scan_interval)
        )

    async def _poll(self, _now=None) -> None:
        await self._refresh()

    async def _refresh(self) -> None:
        try:
            async with self._session.get(
                f"{self._base_url}/status", timeout=aiohttp.ClientTimeout(total=5)
            ) as r:
                r.raise_for_status()
                self._status = await r.json()
                self._attr_available = True
        except Exception:
            self._attr_available = False
        self.async_write_ha_state()

    async def _post(self, path: str, body: dict) -> None:
        async with self._session.post(
            f"{self._base_url}{path}",
            json=body,
            timeout=aiohttp.ClientTimeout(total=5),
        ) as r:
            r.raise_for_status()

    async def _fetch_input_names(self) -> dict[int, str]:
        names = {}
        for i in range(1, 8):
            try:
                async with self._session.get(
                    f"{self._base_url}/input/{i}/name", timeout=aiohttp.ClientTimeout(total=5)
                ) as r:
                    data = await r.json()
                    names[i] = data.get("name", f"Input {i}")
            except Exception:
                names[i] = f"Input {i}"
        return names

    # ---- State properties ----

    @property
    def state(self) -> MediaPlayerState:
        return MediaPlayerState.ON if self._status.get("power") else MediaPlayerState.OFF

    @property
    def volume_level(self) -> float | None:
        v = self._status.get("volume")
        return v / VOLUME_MAX if v is not None else None

    @property
    def is_volume_muted(self) -> bool | None:
        return self._status.get("mute")

    @property
    def source(self) -> str | None:
        i = self._status.get("input")
        return self._input_names.get(i, f"Input {i}") if i else None

    @property
    def source_list(self) -> list[str]:
        return [self._input_names.get(i, f"Input {i}") for i in range(1, 8)]

    # ---- Commands ----

    async def async_turn_on(self) -> None:
        await self._post("/power", {"state": "on"})
        await self._refresh()

    async def async_turn_off(self) -> None:
        await self._post("/power", {"state": "off"})
        await self._refresh()

    async def async_set_volume_level(self, volume: float) -> None:
        await self._post("/volume", {"level": round(volume * VOLUME_MAX)})
        await self._refresh()

    async def async_volume_up(self) -> None:
        await self._post("/volume", {"step": 1})
        await self._refresh()

    async def async_volume_down(self) -> None:
        await self._post("/volume", {"step": -1})
        await self._refresh()

    async def async_mute_volume(self, mute: bool) -> None:
        await self._post("/mute", {"state": mute})
        await self._refresh()

    async def async_select_source(self, source: str) -> None:
        input_num = next((n for n, name in self._input_names.items() if name == source), None)
        if input_num is None:
            _LOGGER.warning("Unknown source: %s", source)
            return
        await self._post("/input", {"input": input_num})
        await self._refresh()
