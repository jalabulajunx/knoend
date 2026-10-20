// Wi-Fi transport (spec/protocol.md sections 2 and 4): joins the home network, announces
// _knoend._tcp (and the older _openpager._tcp) over mDNS, and accepts one authenticated WebSocket session at ws://<board>/ws.
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

#define LINK_WIFI_KEY_BYTES 32

// Called for each verified packet received over Wi-Fi (NUL-terminated JSON text).
typedef void (*link_wifi_packet_cb)(const char *packet);
// Looks up the pairing key for `companion`; returns false if this board is not paired with it.
typedef bool (*link_wifi_key_cb)(const char *companion, uint8_t key[LINK_WIFI_KEY_BYTES]);
// Called once a session is authenticated, so the protocol can send its identity.
typedef void (*link_wifi_session_cb)(void);

// Joins the network stored in NVS, if any. `device_id` is the board's deviceId (12 hex digits).
esp_err_t link_wifi_start(const char *device_id, link_wifi_packet_cb on_packet,
                          link_wifi_key_cb lookup_key, link_wifi_session_cb on_session);

// Stores new credentials (never logged) and joins that network.
esp_err_t link_wifi_set_credentials(const char *ssid, const char *password);

typedef struct {
    char ssid[33];
    int8_t rssi;    // dBm
    uint8_t channel;
    bool secured;
} link_wifi_network_t;

// Scans for networks (2.4 GHz only: that is all the ESP32-S3 radio hears). Fills up to `max`
// entries, strongest first, one per name, hidden networks left out. Returns the count, or -1.
int link_wifi_scan(link_wifi_network_t *out, int max);

// True while an authenticated companion session is open.
bool link_wifi_session_active(void);

// Sends one packet to the authenticated companion, wrapped in a {ctr, mac, packet} frame.
// Does nothing when no session is open.
void link_wifi_send(const char *packet);
