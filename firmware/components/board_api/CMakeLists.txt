# Header-only contract between the firmware core (main/) and the board profiles.
idf_component_register(INCLUDE_DIRS "include" REQUIRES esp_common)
