#include "link_wifi.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#include "cJSON.h"
#include "esp_check.h"
#include "esp_event.h"
#include "esp_http_server.h"
#include "esp_log.h"
#include "esp_netif.h"
#include "esp_random.h"
#include "esp_wifi.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "mbedtls/md.h"
#include "mdns.h"
#include "nvs.h"

#define NONCE_HEX     32
#define MAC_HEX       64
#define MAX_FRAME     (10 * 1024) // a 4096-byte packet, escaped inside a JSON string, plus fields
#define NVS_NAMESPACE "openpager"

static const char *TAG = "link_wifi";

typedef enum { SESSION_NONE, SESSION_WAIT_PROOF, SESSION_OPEN } session_state_t;

static struct {
    SemaphoreHandle_t lock;
    session_state_t state;
    int fd;
    uint8_t key[LINK_WIFI_KEY_BYTES];
    char companion_nonce[NONCE_HEX + 1];
    char board_nonce[NONCE_HEX + 1];
    char session_id[2 * NONCE_HEX + 1]; // board nonce + companion nonce
    int64_t last_ctr_in;
    uint64_t next_ctr_out;
} session = { .fd = -1 };

static char device_id[13];
static link_wifi_packet_cb packet_cb;
static link_wifi_key_cb key_cb;
static link_wifi_session_cb session_cb;
static httpd_handle_t server;
static bool services_started;
static bool have_network; // only try to connect once credentials exist

// ---- crypto helpers ----

static void hmac_hex(const uint8_t key[LINK_WIFI_KEY_BYTES], const char *message, size_t length,
                     char out[MAC_HEX + 1])
{
    uint8_t digest[32];
    mbedtls_md_hmac(mbedtls_md_info_from_type(MBEDTLS_MD_SHA256), key, LINK_WIFI_KEY_BYTES,
                    (const unsigned char *)message, length, digest);
    for (int i = 0; i < 32; i++) {
        sprintf(out + 2 * i, "%02x", digest[i]);
    }
    out[MAC_HEX] = '\0';
}

// Builds `parts` joined by newlines and returns its HMAC as hex.
static void hmac_lines(const uint8_t key[LINK_WIFI_KEY_BYTES], const char *const *parts, int count,
                       char out[MAC_HEX + 1])
{
    size_t length = 0;
    for (int i = 0; i < count; i++) {
        length += strlen(parts[i]) + 1;
    }
    char *message = malloc(length);
    char *cursor = message;
    for (int i = 0; i < count; i++) {
        const size_t n = strlen(parts[i]);
        memcpy(cursor, parts[i], n);
        cursor += n;
        if (i < count - 1) {
            *cursor++ = '\n';
        }
    }
    hmac_hex(key, message, cursor - message, out);
    free(message);
}

static bool equal_constant_time(const char *a, const char *b, size_t length)
{
    if (a == NULL || b == NULL || strlen(a) != length || strlen(b) != length) {
        return false;
    }
    uint8_t diff = 0;
    for (size_t i = 0; i < length; i++) {
        diff |= (uint8_t)(a[i] ^ b[i]);
    }
    return diff == 0;
}

static bool is_hex(const char *text, size_t length)
{
    if (text == NULL || strlen(text) != length) {
        return false;
    }
    for (size_t i = 0; i < length; i++) {
        if (strchr("0123456789abcdef", text[i]) == NULL) {
            return false;
        }
    }
    return true;
}

// ---- sending ----

typedef struct {
    int fd;
    char *text;
} send_job_t;

static void send_job(void *arg)
{
    send_job_t *job = arg;
    httpd_ws_frame_t frame = {
        .type = HTTPD_WS_TYPE_TEXT,
        .payload = (uint8_t *)job->text,
        .len = strlen(job->text),
        .final = true,
    };
    httpd_ws_send_frame_async(server, job->fd, &frame);
    free(job->text);
    free(job);
}

// Queues `text` to the server task, which owns the socket. Takes ownership of `text`.
static void queue_text(int fd, char *text)
{
    send_job_t *job = malloc(sizeof(*job));
    if (job == NULL || text == NULL) {
        free(job);
        free(text);
        return;
    }
    job->fd = fd;
    job->text = text;
    if (httpd_queue_work(server, send_job, job) != ESP_OK) {
        free(text);
        free(job);
    }
}

void link_wifi_send(const char *packet)
{
    xSemaphoreTake(session.lock, portMAX_DELAY);
    if (session.state != SESSION_OPEN) {
        xSemaphoreGive(session.lock);
        return;
    }
    char ctr[24];
    snprintf(ctr, sizeof(ctr), "%llu", (unsigned long long)session.next_ctr_out);
    const char *parts[] = { session.session_id, ctr, packet };
    char mac[MAC_HEX + 1];
    hmac_lines(session.key, parts, 3, mac);

    cJSON *frame = cJSON_CreateObject();
    cJSON_AddNumberToObject(frame, "ctr", (double)session.next_ctr_out);
    cJSON_AddStringToObject(frame, "mac", mac);
    cJSON_AddStringToObject(frame, "packet", packet);
    session.next_ctr_out++;
    const int fd = session.fd;
    xSemaphoreGive(session.lock);

    queue_text(fd, cJSON_PrintUnformatted(frame));
    cJSON_Delete(frame);
}

bool link_wifi_session_active(void)
{
    return session.state == SESSION_OPEN;
}

// ---- session handling (called on the server task) ----

static void end_session_locked(void)
{
    memset(session.key, 0, sizeof(session.key));
    session.state = SESSION_NONE;
    session.fd = -1;
}

static void close_connection(httpd_req_t *req, const char *why)
{
    ESP_LOGW(TAG, "closing companion connection: %s", why);
    xSemaphoreTake(session.lock, portMAX_DELAY);
    if (session.fd == httpd_req_to_sockfd(req)) {
        end_session_locked();
    }
    xSemaphoreGive(session.lock);
    httpd_sess_trigger_close(req->handle, httpd_req_to_sockfd(req));
}

static void handle_hello(httpd_req_t *req, const cJSON *hello)
{
    const char *companion = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(hello, "companion"));
    const char *nonce = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(hello, "nonce"));
    uint8_t key[LINK_WIFI_KEY_BYTES];

    if (companion == NULL || !is_hex(nonce, NONCE_HEX)) {
        close_connection(req, "bad hello");
        return;
    }
    if (!key_cb(companion, key)) {
        close_connection(req, "not paired with this companion");
        return;
    }

    xSemaphoreTake(session.lock, portMAX_DELAY);
    end_session_locked(); // a new hello replaces any earlier session
    session.fd = httpd_req_to_sockfd(req);
    memcpy(session.key, key, sizeof(key));
    memset(key, 0, sizeof(key));
    strcpy(session.companion_nonce, nonce);
    uint8_t random[NONCE_HEX / 2];
    esp_fill_random(random, sizeof(random));
    for (int i = 0; i < (int)sizeof(random); i++) {
        sprintf(session.board_nonce + 2 * i, "%02x", random[i]);
    }
    const char *parts[] = { "board", session.companion_nonce, session.board_nonce };
    char proof[MAC_HEX + 1];
    hmac_lines(session.key, parts, 3, proof);
    session.state = SESSION_WAIT_PROOF;
    xSemaphoreGive(session.lock);

    cJSON *reply = cJSON_CreateObject();
    cJSON *body = cJSON_AddObjectToObject(reply, "hello");
    cJSON_AddStringToObject(body, "board", device_id);
    cJSON_AddStringToObject(body, "nonce", session.board_nonce);
    cJSON_AddStringToObject(body, "proof", proof);
    queue_text(httpd_req_to_sockfd(req), cJSON_PrintUnformatted(reply));
    cJSON_Delete(reply);
}

static void handle_proof(httpd_req_t *req, const char *proof)
{
    xSemaphoreTake(session.lock, portMAX_DELAY);
    const bool expected_state = session.state == SESSION_WAIT_PROOF && session.fd == httpd_req_to_sockfd(req);
    char expected[MAC_HEX + 1] = "";
    if (expected_state) {
        const char *parts[] = { "companion", session.board_nonce, session.companion_nonce };
        hmac_lines(session.key, parts, 3, expected);
    }
    const bool ok = expected_state && equal_constant_time(proof, expected, MAC_HEX);
    if (ok) {
        snprintf(session.session_id, sizeof(session.session_id), "%s%s", session.board_nonce,
                 session.companion_nonce);
        session.last_ctr_in = -1;
        session.next_ctr_out = 0;
        session.state = SESSION_OPEN;
    }
    xSemaphoreGive(session.lock);

    if (!ok) {
        close_connection(req, "companion proof does not verify");
        return;
    }
    printf("WIFI session open\n");
    fflush(stdout);
    session_cb();
}

static void handle_frame(httpd_req_t *req, const cJSON *message)
{
    const cJSON *ctr = cJSON_GetObjectItemCaseSensitive(message, "ctr");
    const char *mac = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(message, "mac"));
    const char *packet = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(message, "packet"));
    if (!cJSON_IsNumber(ctr) || ctr->valuedouble < 0 || packet == NULL) {
        close_connection(req, "bad frame");
        return;
    }

    xSemaphoreTake(session.lock, portMAX_DELAY);
    bool ok = session.state == SESSION_OPEN && session.fd == httpd_req_to_sockfd(req);
    if (ok) {
        char counter[24];
        snprintf(counter, sizeof(counter), "%llu", (unsigned long long)ctr->valuedouble);
        const char *parts[] = { session.session_id, counter, packet };
        char expected[MAC_HEX + 1];
        hmac_lines(session.key, parts, 3, expected);
        ok = equal_constant_time(mac, expected, MAC_HEX) && (int64_t)ctr->valuedouble > session.last_ctr_in;
        if (ok) {
            session.last_ctr_in = (int64_t)ctr->valuedouble;
        }
    }
    xSemaphoreGive(session.lock);

    if (!ok) {
        close_connection(req, "frame failed authentication or replayed");
        return;
    }
    packet_cb(packet);
}

static esp_err_t ws_handler(httpd_req_t *req)
{
    if (req->method == HTTP_GET) {
        ESP_LOGI(TAG, "companion connected (fd %d)", httpd_req_to_sockfd(req));
        return ESP_OK; // handshake upgrade done; the hello comes as the first frame
    }

    httpd_ws_frame_t frame = { .type = HTTPD_WS_TYPE_TEXT };
    ESP_RETURN_ON_ERROR(httpd_ws_recv_frame(req, &frame, 0), TAG, "frame length");
    if (frame.type != HTTPD_WS_TYPE_TEXT || frame.len == 0 || frame.len > MAX_FRAME) {
        close_connection(req, "frame type or size not allowed");
        return ESP_OK;
    }
    char *text = calloc(1, frame.len + 1);
    if (text == NULL) {
        return ESP_ERR_NO_MEM;
    }
    frame.payload = (uint8_t *)text;
    esp_err_t err = httpd_ws_recv_frame(req, &frame, frame.len);
    if (err != ESP_OK) {
        free(text);
        return err;
    }

    cJSON *message = cJSON_Parse(text);
    const cJSON *hello = cJSON_GetObjectItemCaseSensitive(message, "hello");
    const cJSON *proof = cJSON_GetObjectItemCaseSensitive(message, "proof");
    if (message == NULL) {
        close_connection(req, "not JSON");
    } else if (cJSON_IsObject(hello)) {
        handle_hello(req, hello);
    } else if (cJSON_IsString(proof)) {
        handle_proof(req, proof->valuestring);
    } else {
        handle_frame(req, message);
    }
    cJSON_Delete(message);
    free(text);
    return ESP_OK;
}

static void on_socket_close(httpd_handle_t handle, int fd)
{
    xSemaphoreTake(session.lock, portMAX_DELAY);
    if (session.fd == fd) {
        end_session_locked();
        printf("WIFI session closed\n");
        fflush(stdout);
    }
    xSemaphoreGive(session.lock);
    close(fd);
}

// ---- network ----

static void start_services(void)
{
    if (services_started) {
        return;
    }
    services_started = true;

    char hostname[32];
    snprintf(hostname, sizeof(hostname), "openpager-%s", device_id + 6);
    if (mdns_init() == ESP_OK) {
        mdns_hostname_set(hostname);
        mdns_instance_name_set("OpenPager");
        mdns_txt_item_t txt[] = { { "id", device_id }, { "pv", "0" } };
        mdns_service_add(NULL, "_openpager", "_tcp", 80, txt, 2);
    } else {
        ESP_LOGE(TAG, "mDNS did not start");
    }

    httpd_config_t config = HTTPD_DEFAULT_CONFIG();
    config.stack_size = 8192;
    config.close_fn = on_socket_close;
    config.lru_purge_enable = true;
    if (httpd_start(&server, &config) != ESP_OK) {
        ESP_LOGE(TAG, "web server did not start");
        return;
    }
    static const httpd_uri_t ws = {
        .uri = "/ws", .method = HTTP_GET, .handler = ws_handler, .is_websocket = true,
    };
    httpd_register_uri_handler(server, &ws);
    printf("WIFI services up: %s.local, _openpager._tcp port 80\n", hostname);
    fflush(stdout);
}

static void on_wifi_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        if (!have_network) {
            return;
        }
        const wifi_event_sta_disconnected_t *event = data;
        printf("WIFI disconnected reason=%d, retrying\n", event->reason);
        fflush(stdout);
        vTaskDelay(pdMS_TO_TICKS(2000));
        esp_wifi_connect();
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        const ip_event_got_ip_t *event = data;
        printf("WIFI ip=" IPSTR "\n", IP2STR(&event->ip_info.ip));
        fflush(stdout);
        start_services();
    }
}

static esp_err_t load_credentials(wifi_config_t *config)
{
    nvs_handle_t nvs;
    ESP_RETURN_ON_ERROR(nvs_open(NVS_NAMESPACE, NVS_READONLY, &nvs), TAG, "no stored network");
    size_t ssid_len = sizeof(config->sta.ssid);
    size_t pass_len = sizeof(config->sta.password);
    esp_err_t err = nvs_get_str(nvs, "wifi_ssid", (char *)config->sta.ssid, &ssid_len);
    if (err == ESP_OK) {
        err = nvs_get_str(nvs, "wifi_pass", (char *)config->sta.password, &pass_len);
    }
    nvs_close(nvs);
    return err;
}

static esp_err_t join(const wifi_config_t *stored)
{
    wifi_config_t config = *stored;
    config.sta.threshold.authmode = config.sta.password[0] ? WIFI_AUTH_WPA2_PSK : WIFI_AUTH_OPEN;
    ESP_RETURN_ON_ERROR(esp_wifi_set_config(WIFI_IF_STA, &config), TAG, "set config");
    printf("WIFI joining \"%s\"\n", (const char *)config.sta.ssid);
    fflush(stdout);
    have_network = true;
    esp_wifi_disconnect();
    return esp_wifi_connect();
}

esp_err_t link_wifi_set_credentials(const char *ssid, const char *password)
{
    if (ssid == NULL || ssid[0] == '\0' || strlen(ssid) > 32 || password == NULL || strlen(password) > 64) {
        return ESP_ERR_INVALID_ARG;
    }
    nvs_handle_t nvs;
    ESP_RETURN_ON_ERROR(nvs_open(NVS_NAMESPACE, NVS_READWRITE, &nvs), TAG, "nvs");
    esp_err_t err = nvs_set_str(nvs, "wifi_ssid", ssid);
    if (err == ESP_OK) {
        err = nvs_set_str(nvs, "wifi_pass", password);
    }
    if (err == ESP_OK) {
        err = nvs_commit(nvs);
    }
    nvs_close(nvs);
    ESP_RETURN_ON_ERROR(err, TAG, "store credentials");

    wifi_config_t config = { 0 };
    memcpy(config.sta.ssid, ssid, strlen(ssid));
    memcpy(config.sta.password, password, strlen(password));
    err = join(&config);
    memset(&config, 0, sizeof(config));
    return err;
}

int link_wifi_scan(link_wifi_network_t *out, int max)
{
    const wifi_scan_config_t config = { .show_hidden = false };
    esp_err_t err = ESP_FAIL;
    // A scan is refused while the radio is in the middle of connecting; try a few times.
    for (int attempt = 0; attempt < 4 && err != ESP_OK; attempt++) {
        err = esp_wifi_scan_start(&config, true);
        if (err != ESP_OK) {
            vTaskDelay(pdMS_TO_TICKS(800));
        }
    }
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "scan failed: %s", esp_err_to_name(err));
        return -1;
    }
    uint16_t found = 0;
    esp_wifi_scan_get_ap_num(&found);
    wifi_ap_record_t *records = calloc(found ? found : 1, sizeof(*records));
    if (records == NULL) {
        esp_wifi_clear_ap_list();
        return -1;
    }
    esp_wifi_scan_get_ap_records(&found, records); // sorted strongest first

    int count = 0;
    for (int i = 0; i < found && count < max; i++) {
        const char *ssid = (const char *)records[i].ssid;
        bool seen = ssid[0] == '\0';
        for (int j = 0; j < count && !seen; j++) {
            seen = strcmp(out[j].ssid, ssid) == 0;
        }
        if (seen) {
            continue;
        }
        snprintf(out[count].ssid, sizeof(out[count].ssid), "%s", ssid);
        out[count].rssi = records[i].rssi;
        out[count].channel = records[i].primary;
        out[count].secured = records[i].authmode != WIFI_AUTH_OPEN;
        count++;
    }
    free(records);
    return count;
}

esp_err_t link_wifi_start(const char *id, link_wifi_packet_cb on_packet, link_wifi_key_cb lookup_key,
                          link_wifi_session_cb on_session)
{
    snprintf(device_id, sizeof(device_id), "%s", id);
    packet_cb = on_packet;
    key_cb = lookup_key;
    session_cb = on_session;
    session.lock = xSemaphoreCreateMutex();
    ESP_RETURN_ON_FALSE(session.lock != NULL, ESP_ERR_NO_MEM, TAG, "lock");

    ESP_RETURN_ON_ERROR(esp_netif_init(), TAG, "netif");
    esp_err_t err = esp_event_loop_create_default();
    if (err != ESP_OK && err != ESP_ERR_INVALID_STATE) {
        return err;
    }
    esp_netif_t *netif = esp_netif_create_default_wifi_sta();
    char hostname[32];
    snprintf(hostname, sizeof(hostname), "openpager-%s", device_id + 6);
    esp_netif_set_hostname(netif, hostname);

    wifi_init_config_t init = WIFI_INIT_CONFIG_DEFAULT();
    ESP_RETURN_ON_ERROR(esp_wifi_init(&init), TAG, "wifi init");
    ESP_RETURN_ON_ERROR(esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, on_wifi_event, NULL), TAG, "events");
    ESP_RETURN_ON_ERROR(esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, on_wifi_event, NULL), TAG, "events");
    ESP_RETURN_ON_ERROR(esp_wifi_set_storage(WIFI_STORAGE_RAM), TAG, "storage"); // ours, in NVS
    ESP_RETURN_ON_ERROR(esp_wifi_set_mode(WIFI_MODE_STA), TAG, "mode");
    ESP_RETURN_ON_ERROR(esp_wifi_start(), TAG, "start");

    wifi_config_t stored = { 0 };
    if (load_credentials(&stored) == ESP_OK) {
        join(&stored);
        memset(&stored, 0, sizeof(stored));
    } else {
        printf("WIFI no network stored; send openpager.wifi over USB\n");
        fflush(stdout);
    }
    return ESP_OK;
}
